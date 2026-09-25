#!/usr/bin/env python3
"""Head-to-head comparison of StepCheck (native) with formal workflow-soundness verifiers
(Woflan, van der Aalst & Verbeek, run locally via pm4py; optionally BPMN
Analyzer 2.0 and BProVe/BPMNOS) on the ASL->BPMN control-flow fair class.

Methodology (confound-free, mirrors eval/validator_panel.js):
  * control  = `stepcheck emit  <wf>`          (canonical re-emit, one file)
  * mutant_k = `stepcheck mutate <wf> --kind k` (control + exactly one injected
               defect of class k)
  The only difference between control and mutant_k is the injected defect, so a
  NEW soundness violation on mutant_k is attributable to it.

For each workflow and each applicable class k we record:
  * native_detected  : StepCheck's `check` reports the class's expected SC code.
  * woflan_detected  : the BPMN encoding of mutant_k carries a soundness
                       violation absent from the encoding of the control
                       (a fresh negative Woflan verdict, or a net that no longer
                       builds as a workflow net -- e.g. a dangling transition).
    * bpmn_analyzer_detected / bprove_detected: analogous fresh violations when
                                             those optional academic baselines are configured.

We also record, per control, whether Woflan already reports the clean workflow
unsound: the verifier's practitioner-visible over-report rate on valid AWS
serverless workflows (a mismatch between classical WF-net soundness and ASL's
multi-terminal / independent-branch semantics).

Outputs eval/asl2bpmn-comparison.json with per-class recall for both tools and
bootstrap CIs, plus the clean-baseline over-report rate.
"""
import sys, os, io, json, glob, subprocess, contextlib, random, argparse, warnings, tempfile, re, shlex
warnings.filterwarnings("ignore")

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ROOT = os.path.dirname(ROOT)  # repo root (eval/asl2bpmn -> eval -> root)

_s = sys.stderr; sys.stderr = io.StringIO()
import pm4py
from pm4py.algo.analysis.woflan import algorithm as woflan
sys.stderr = _s

EXPECTED = {  # mutation class -> the SC code that must catch it
    "structural": "SC0002",
    "contract": "SC1003",
    "retry": "SC3001",
    "compensation": "SC4001",
    "concurrency": "SC5001",
    "temporal": "SC6003",
}
# Analysis-family codes (siblings) that also legitimately catch a class. Used by
# the hard-mutant study, where a boundary defect may be caught by a sibling
# (e.g. a non-compensating Catch by SC4010 rather than SC4001).
FAMILY = {
    "structural": ["SC0002"],
    "contract": ["SC1003"],
    "retry": ["SC3001"],
    "compensation": ["SC4001", "SC4010", "SC4011"],
    "concurrency": ["SC5001", "SC5002", "SC5003"],
    "temporal": ["SC6003"],
}
CLASS_LABEL = {
    "structural": "Dangling transition (SC0002)",
    "contract": "Broken data binding (SC1003)",
    "retry": "Unsafe retry (SC3001)",
    "compensation": "Missing compensation (SC4001)",
    "concurrency": "Concurrency interference (SC5001)",
    "temporal": "Heartbeat/timeout (SC6003)",
}
# Which classes are, in principle, control-flow-soundness properties (the only
# ones a WF-net verifier could express). Everything else is StepCheck-unique.
CONTROL_FLOW_CLASS = {"structural"}

POSITIVE = {
    "Input is ok.",
    "Petri Net is a workflow net.",
    "Every place is covered by s-components.",
    "There are no dead tasks.",
    "All tasks are live.",
    "No improper coditions.",
    "No improper conditions.",
}

def norm_msg(m):
    """Category key: drop the specific [id,...] list / trailing numbers."""
    for sep in (":", "["):
        i = m.find(sep)
        if i != -1:
            m = m[:i]
    return m.strip().rstrip(".").strip()

def negatives(msgs):
    return {norm_msg(m) for m in msgs if m not in POSITIVE and norm_msg(m) not in {norm_msg(p) for p in POSITIVE}}

def run_stepcheck(binexe, args):
    try:
        p = subprocess.run([binexe] + args, capture_output=True, text=True, timeout=120)
        return p.stdout, p.returncode
    except Exception as e:
        return "", -1

def native_codes(binexe, aslpath, result_shapes=False):
    """Per-code diagnostic *counts* (dict code->n). A fresh detection is a count
    increase over the control, so a newly-injected instance of a code counts even
    when that code already fires elsewhere in the workflow (matching the native
    hard study's count-based fresh criterion). `code in counts` still works for the
    easy presence check."""
    args = ["check", "--json", "--infer"]
    if result_shapes:
        args.append("--result-shapes")
    out, _ = run_stepcheck(binexe, args + [aslpath])
    try:
        counts = {}
        for d in json.loads(out).get("diagnostics", []):
            counts[d["code"]] = counts.get(d["code"], 0) + 1
        return counts
    except Exception:
        return None

def encode(node, encodejs, aslpath, bpmnpath):
    try:
        with open(bpmnpath, "wb") as f:
            p = subprocess.run([node, encodejs, aslpath], stdout=f, stderr=subprocess.PIPE, timeout=60)
        return p.returncode  # 0 = valid WF-net; 1 = validation failed (e.g. dangling)
    except Exception:
        return -1

def woflan_verdict(bpmnpath):
    """Returns (sound|'build_error'|'woflan_error', messages). In-process: the
    encoded nets are <=30 states, so Woflan returns in well under a second; the
    per-net multiprocessing isolation cost (re-importing pm4py under Windows
    spawn) is not worth paying."""
    try:
        with contextlib.redirect_stderr(io.StringIO()):
            bpmn = pm4py.read_bpmn(bpmnpath)
            net, im, fm = pm4py.convert_to_petri_net(bpmn)
    except Exception as e:
        return ("build_error", [type(e).__name__])
    try:
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            sound, det = woflan.apply(net, im, fm, parameters={
                woflan.Parameters.RETURN_ASAP_WHEN_NOT_SOUND: False,
                woflan.Parameters.PRINT_DIAGNOSTICS: False,
                woflan.Parameters.RETURN_DIAGNOSTICS: True})
        msgs = []
        for k, v in det.items():
            if getattr(k, "value", k) == "diagnostic_messages":
                msgs = list(v)
        return (bool(sound), msgs)
    except Exception as e:
        return ("woflan_error", [type(e).__name__])

def woflan_detected(control, mutant):
    """Fresh soundness violation on the mutant vs the control."""
    cs, cm = control
    ms, mm = mutant
    # mutant no longer builds / verifies as a workflow net, control did
    if ms in ("build_error", "woflan_error", "timeout") and cs not in ("build_error", "woflan_error", "timeout"):
        return True
    if cs is True and ms is False:
        return True
    cn, mn = negatives(cm), negatives(mm)
    return len(mn - cn) > 0

# --- Second formal competitor: BPMN Analyzer 2.0 (Kraeuter 2024), a BPMN-specific
# model checker in Rust, run via its CLI. Checks the same soundness sub-properties
# (safeness, option-to-complete, proper-completion, no-dead-activities) and returns
# a counterexample trace on failure. Configure the binary via --bpmn-analyzer or
# the BPMN_ANALYZER env var; when absent, this competitor is skipped.
BPMN_ANALYZER = os.environ.get("BPMN_ANALYZER")
BA_PROPS = "safeness,option-to-complete,proper-completion,no-dead-activities"

def bpmn_analyzer_verdict(bpmnpath):
    """Returns (sound|'build_error', violated_property_set)."""
    if not BPMN_ANALYZER:
        return (None, set())
    try:
        p = subprocess.run([BPMN_ANALYZER, "-f", bpmnpath, "-p", BA_PROPS],
                           capture_output=True, text=True, timeout=90)
    except Exception:
        return ("build_error", set())
    out = p.stdout + p.stderr
    if p.returncode != 0 or "Application error" in out or "State space" not in out:
        return ("build_error", set())
    violated = set()
    for prop, label in [("safeness", "Safeness"), ("option-to-complete", "Option to complete"),
                        ("proper-completion", "Proper completion"), ("no-dead-activities", "No dead activities")]:
        if f"{label} is not fulfilled" in out:
            violated.add(prop)
    return (len(violated) == 0, violated)

def bpmn_analyzer_detected(control, mutant):
    cs, cv = control
    ms, mv = mutant
    if ms == "build_error" and cs != "build_error":
        return True
    if cs is True and ms is False:
        return True
    if isinstance(cv, set) and isinstance(mv, set):
        return len(mv - cv) > 0
    return False

# --- Third formal competitor: BProVe/BPMNOS (Corradini et al.), run locally as
# the BPMNOS Java BPMN parser plus the Maude model checker. BProVe's reference
# driver generates LTL checks over pool reachability/completion properties; we
# use the documented soundness-style checks from BPMNOS_STARTER.maude.
BPROVE_PARSER = os.environ.get("BPROVE_PARSER") or os.environ.get("BPROVE_PARSER_JAR")
BPROVE_MAUDE_MODEL = os.environ.get("BPROVE_MAUDE_MODEL")
if not BPROVE_MAUDE_MODEL and os.environ.get("BPROVE_MAUDE_DIR"):
    BPROVE_MAUDE_MODEL = os.path.join(os.environ["BPROVE_MAUDE_DIR"], "BPMNOS_MODEL_CHECKER.maude")
BPROVE_TIMEOUT = int(os.environ.get("BPROVE_TIMEOUT", "90"))
BPROVE_ERROR = {"build_error", "maude_error", "timeout"}

def wsl_path(path):
    path = os.path.abspath(path)
    if os.name != "nt":
        return path
    drive, rest = os.path.splitdrive(path)
    if not drive:
        return path.replace("\\", "/")
    rest = rest.replace("\\", "/")
    return f"/mnt/{drive[0].lower()}{rest}"

def bprove_configured():
    return bool(BPROVE_PARSER and BPROVE_MAUDE_MODEL)

def bprove_parser_output(base):
    candidates = [base, base + ".txt", base + ".txt.txt"]
    existing = [p for p in candidates if os.path.exists(p)]
    if not existing:
        return None
    return max(existing, key=os.path.getmtime)

def bprove_quote(s):
    return '"' + s.replace('\\', '\\\\').replace('"', '\\"') + '"'

def bprove_formulas(pools):
    formulas = []
    for pool in pools:
        q = bprove_quote(pool)
        formulas.extend([
            ("safeness", f"[] (safeness({q}))"),
            ("option-to-complete", f"[] (aBPoolstartsParameterized({q}) -> <> aBPoolendsParameterized({q}))"),
            ("proper-completion", f"[] (aBPoolendsParameterized({q}) -> noMultipleTokenAround({q}))"),
        ])
    return formulas

def bprove_parse_results(text):
    results = []
    for line in text.splitlines():
        m = re.search(r"result Bool:\s*(true|false)", line)
        if m:
            results.append(m.group(1) == "true")
            continue
        m = re.search(r"result (?:ModelCheckResult|\[ModelCheckResult\]):\s*(.*)", line)
        if not m:
            continue
        value = m.group(1).strip()
        if value.startswith("true"):
            results.append(True)
        elif value.startswith("false") or value.startswith("counterexample"):
            results.append(False)
        else:
            results.append(None)
    return results

def run_maude(script, timeout):
    if os.name == "nt":
        cmd = ["wsl.exe", "bash", "-lc", "maude " + shlex.quote(wsl_path(script))]
    else:
        cmd = ["maude", script]
    return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)

def bprove_verdict(bpmnpath):
    """Returns (sound|'build_error'|'maude_error'|'timeout', violated_properties)."""
    if not bprove_configured():
        return (None, set())
    base = os.path.join(os.path.dirname(bpmnpath), os.path.basename(bpmnpath) + ".bpmnos")
    for p in (base, base + ".txt", base + ".txt.txt"):
        try:
            os.remove(p)
        except OSError:
            pass
    try:
        parser = os.path.abspath(BPROVE_PARSER)
        p = subprocess.run(["java", "-jar", parser, bpmnpath, base],
                           cwd=os.path.dirname(parser), capture_output=True, text=True, timeout=60)
    except Exception:
        return ("build_error", {"parser"})
    term_path = bprove_parser_output(base)
    if not term_path:
        return ("build_error", {"parser"})
    try:
        term = open(term_path, encoding="utf-8", errors="replace").read().strip()
    except OSError:
        return ("build_error", {"parser"})
    if "Not eligible elements" in term or "Parsing Not Succeded" in (p.stdout + p.stderr) or not term.startswith("collaboration("):
        return ("build_error", {"parser"})
    pools = sorted(set(re.findall(r'pool\(\s*"([^"]+)"', term)))
    if not pools:
        return ("build_error", {"parser"})
    formulas = bprove_formulas(pools)
    script = base + ".maude"
    lines = ["load " + wsl_path(BPROVE_MAUDE_MODEL)]
    for _, formula in formulas:
        lines.extend(["red modelCheck(", term, f", {formula} ) ."])
    lines.append("quit")
    try:
        with open(script, "w", encoding="ascii") as f:
            f.write("\n".join(lines) + "\n")
        p = run_maude(script, BPROVE_TIMEOUT)
    except subprocess.TimeoutExpired:
        return ("timeout", set())
    except Exception:
        return ("maude_error", set())
    out = p.stdout + p.stderr
    if p.returncode != 0 or "no parse for term" in out or "bad token" in out or "unable to locate file" in out:
        return ("maude_error", set())
    results = bprove_parse_results(out)
    if len(results) < len(formulas) or any(r is None for r in results):
        return ("maude_error", set())
    violated = {label for (label, _), ok in zip(formulas, results) if ok is False}
    return (len(violated) == 0, violated)

def bprove_detected(control, mutant):
    cs, cv = control
    ms, mv = mutant
    if ms in BPROVE_ERROR and cs not in BPROVE_ERROR:
        return True
    if cs is True and ms is False:
        return True
    if isinstance(cv, set) and isinstance(mv, set):
        return len(mv - cv) > 0
    return False

def bootstrap_ci(flags, B=3000, seed=17):
    """95% percentile CI for the mean of a 0/1 list (recall)."""
    if not flags:
        return [None, None]
    rng = random.Random(seed)
    n = len(flags)
    means = []
    for _ in range(B):
        s = sum(flags[rng.randrange(n)] for _ in range(n))
        means.append(s / n)
    means.sort()
    lo = means[int(0.025 * B)]
    hi = means[int(0.975 * B)]
    return [round(lo, 3), round(hi, 3)]

def main():
    global BPMN_ANALYZER, BPROVE_PARSER, BPROVE_MAUDE_MODEL, BPROVE_TIMEOUT
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", default=os.path.join(ROOT, "corpus", "asl"))
    ap.add_argument("--include", default=None, help="optional regex over relative input paths")
    ap.add_argument("--limit", type=int, default=30)
    ap.add_argument("--bin", default=os.path.join(ROOT, "stepcheck", "target", "release", "stepcheck.exe"))
    ap.add_argument("--node", default="node")
    ap.add_argument("--encode", default=os.path.join(ROOT, "eval", "asl2bpmn", "encode.js"))
    ap.add_argument("--out", default=None)
    ap.add_argument("--hard", action="store_true",
                    help="inject the hard (⊤/coverage-boundary) mutant of each class and credit StepCheck at the analysis-family level")
    ap.add_argument("--bpmn-analyzer", default=BPMN_ANALYZER, help="path to rust_bpmn_analyzer_cli(.exe)")
    ap.add_argument("--bprove-parser", default=BPROVE_PARSER, help="path to BPMNOS_Parser.jar")
    ap.add_argument("--bprove-maude-model", default=BPROVE_MAUDE_MODEL, help="path to BPMNOS_MODEL_CHECKER.maude")
    ap.add_argument("--bprove-timeout", type=int, default=BPROVE_TIMEOUT, help="per-model BProVe timeout in seconds")
    args = ap.parse_args()
    BPMN_ANALYZER = args.bpmn_analyzer
    BPROVE_PARSER = args.bprove_parser
    BPROVE_MAUDE_MODEL = args.bprove_maude_model
    BPROVE_TIMEOUT = args.bprove_timeout
    if args.out is None:
        args.out = os.path.join(ROOT, "eval",
                                "asl2bpmn-comparison-hard.json" if args.hard else "asl2bpmn-comparison.json")

    files = sorted(glob.glob(os.path.join(args.corpus, "**", "*.json"), recursive=True))
    if args.include:
        inc = re.compile(args.include)
        files = [f for f in files if inc.search(os.path.relpath(f, ROOT).replace("\\", "/"))]
    files = files[: args.limit]
    tmp = tempfile.mkdtemp(prefix="wsd-")
    print(f"[compare] {len(files)} workflows; tmp={tmp}; bpmn_analyzer={'on' if BPMN_ANALYZER else 'off'}; bprove={'on' if bprove_configured() else 'off'}", file=sys.stderr)

    per_class = {k: {"applicable": 0, "native": 0, "woflan": 0, "ba": 0, "bprove": 0,
                     "native_flags": [], "woflan_flags": [], "ba_flags": [], "bprove_flags": []} for k in EXPECTED}
    clean_unsound = 0
    clean_ba_unsound = 0
    clean_bprove_unsound = 0
    clean_total = 0
    control_verdicts = []
    rows = []

    for idx, f in enumerate(files):
        base = os.path.splitext(os.path.basename(f))[0]
        ctl_asl = os.path.join(tmp, f"{base}.control.json")
        ctl_bpmn = os.path.join(tmp, f"{base}.control.bpmn")
        out, rc = run_stepcheck(args.bin, ["emit", f, "--out", ctl_asl])
        if rc != 0 or not os.path.exists(ctl_asl):
            print(f"  emit failed: {base}", file=sys.stderr); continue
        # Control StepCheck codes, so hard-mutant detection is credited only for a
        # *fresh* finding (matching the native fresh criterion), not a pre-existing one.
        ctl_counts = native_codes(args.bin, ctl_asl, result_shapes=args.hard) or {}
        encode(args.node, args.encode, ctl_asl, ctl_bpmn)
        control = woflan_verdict(ctl_bpmn)
        control_ba = bpmn_analyzer_verdict(ctl_bpmn)
        control_bprove = bprove_verdict(ctl_bpmn)
        clean_total += 1
        cs = control[0]
        is_unsound = (cs is False) or (cs in ("build_error", "woflan_error", "timeout"))
        if is_unsound:
            clean_unsound += 1
        if control_ba[0] is False or control_ba[0] == "build_error":
            clean_ba_unsound += 1
        if control_bprove[0] is False or control_bprove[0] in BPROVE_ERROR:
            clean_bprove_unsound += 1
        control_verdicts.append({"file": os.path.relpath(f, ROOT).replace("\\", "/"),
                                 "sound": cs if isinstance(cs, bool) else str(cs),
                                 "neg": sorted(negatives(control[1])),
                                 "bpmn_analyzer_sound": control_ba[0] if isinstance(control_ba[0], bool) else str(control_ba[0]),
                                 "bprove_sound": control_bprove[0] if isinstance(control_bprove[0], bool) else str(control_bprove[0]),
                                 "bprove_violated": sorted(control_bprove[1])})

        row = {"file": os.path.relpath(f, ROOT).replace("\\", "/"),
               "control_sound": cs if isinstance(cs, bool) else str(cs),
               "control_bprove_sound": control_bprove[0] if isinstance(control_bprove[0], bool) else str(control_bprove[0]),
               "classes": {}}
        for k in EXPECTED:
            mut_asl = os.path.join(tmp, f"{base}.{k}.json")
            mut_args = ["mutate", f, "--kind", k, "--out", mut_asl]
            if args.hard:
                mut_args.append("--hard")
            out, rc = run_stepcheck(args.bin, mut_args)
            if rc != 0 or not os.path.exists(mut_asl):
                continue  # no applicable site
            per_class[k]["applicable"] += 1
            codes = native_codes(args.bin, mut_asl, result_shapes=args.hard) or {}
            # Hard mutants credit any *fresh* sibling in the class's analysis
            # family: a count increase over the control (matching the native
            # study); easy mutants require the single expected code (unchanged).
            if args.hard:
                nat = any(codes.get(c, 0) > ctl_counts.get(c, 0) for c in FAMILY[k])
            else:
                nat = EXPECTED[k] in codes
            mut_bpmn = os.path.join(tmp, f"{base}.{k}.bpmn")
            encode(args.node, args.encode, mut_asl, mut_bpmn)
            mutant = woflan_verdict(mut_bpmn)
            wof = woflan_detected(control, mutant)
            mutant_ba = bpmn_analyzer_verdict(mut_bpmn)
            ba = bpmn_analyzer_detected(control_ba, mutant_ba) if BPMN_ANALYZER else False
            mutant_bprove = bprove_verdict(mut_bpmn)
            bp = bprove_detected(control_bprove, mutant_bprove) if bprove_configured() else False
            per_class[k]["native"] += int(nat)
            per_class[k]["woflan"] += int(wof)
            per_class[k]["ba"] += int(ba)
            per_class[k]["bprove"] += int(bp)
            per_class[k]["native_flags"].append(int(nat))
            per_class[k]["woflan_flags"].append(int(wof))
            per_class[k]["ba_flags"].append(int(ba))
            per_class[k]["bprove_flags"].append(int(bp))
            row["classes"][k] = {"native": nat, "woflan": wof, "bpmn_analyzer": ba, "bprove": bp,
                                 "mut_sound": mutant[0] if isinstance(mutant[0], bool) else str(mutant[0]),
                                 "mut_bprove_sound": mutant_bprove[0] if isinstance(mutant_bprove[0], bool) else str(mutant_bprove[0]),
                                 "mut_bprove_violated": sorted(mutant_bprove[1])}
        rows.append(row)
        print(f"  [{idx+1}/{len(files)}] {base[:48]:48} control={row['control_sound']}", file=sys.stderr)

    classes_out = {}
    for k, d in per_class.items():
        n = d["applicable"]
        classes_out[k] = {
            "label": CLASS_LABEL[k],
            "expected_code": EXPECTED[k],
            "control_flow_expressible": k in CONTROL_FLOW_CLASS,
            "applicable": n,
            "stepcheck_detected": d["native"],
            "stepcheck_recall": round(d["native"] / n, 3) if n else None,
            "stepcheck_ci95": bootstrap_ci(d["native_flags"]),
            "woflan_detected": d["woflan"],
            "woflan_recall": round(d["woflan"] / n, 3) if n else None,
            "woflan_ci95": bootstrap_ci(d["woflan_flags"]),
            "bpmn_analyzer_detected": d["ba"] if BPMN_ANALYZER else None,
            "bpmn_analyzer_recall": (round(d["ba"] / n, 3) if n else None) if BPMN_ANALYZER else None,
            "bpmn_analyzer_ci95": bootstrap_ci(d["ba_flags"]) if BPMN_ANALYZER else [None, None],
            "bprove_detected": d["bprove"] if bprove_configured() else None,
            "bprove_recall": (round(d["bprove"] / n, 3) if n else None) if bprove_configured() else None,
            "bprove_ci95": bootstrap_ci(d["bprove_flags"]) if bprove_configured() else [None, None],
        }

    report = {
        "generated_by": "eval/asl2bpmn/compare.py",
        "hard_mutants": args.hard,
        "stepcheck_crediting": "analysis-family (siblings)" if args.hard else "single expected code",
        "verifier": "Woflan (van der Aalst & Verbeek) via pm4py " + pm4py.__version__,
        "verifier2": "BPMN Analyzer 2.0 (Kraeuter 2024) via rust_bpmn_analyzer CLI" if BPMN_ANALYZER else "not configured",
        "verifier3": "BProVe/BPMNOS (Corradini et al.) via BPMNOS parser + Maude" if bprove_configured() else "not configured",
        "bprove_properties": ["safeness", "option-to-complete", "proper-completion"] if bprove_configured() else [],
        "corpus": os.path.relpath(args.corpus, ROOT).replace("\\", "/"),
        "workflows": len(rows),
        "methodology": "confound-free: control=`stepcheck emit`, mutant=`stepcheck mutate --kind k`; "
                       "a formal verifier detects the class iff the mutant encoding carries a fresh "
                       "soundness violation absent from the control encoding.",
        "clean_baseline": {
            "workflows": clean_total,
            "woflan_unsound": clean_unsound,
            "woflan_overreport_rate": round(clean_unsound / clean_total, 3) if clean_total else None,
            "bpmn_analyzer_unsound": clean_ba_unsound if BPMN_ANALYZER else None,
            "bpmn_analyzer_overreport_rate": (round(clean_ba_unsound / clean_total, 3) if clean_total else None) if BPMN_ANALYZER else None,
            "bprove_unsound": clean_bprove_unsound if bprove_configured() else None,
            "bprove_overreport_rate": (round(clean_bprove_unsound / clean_total, 3) if clean_total else None) if bprove_configured() else None,
            "note": "A formal workflow verifier flags a valid AWS serverless workflow unsound when its "
                    "parallel-branch termination is not well-handled (multi-terminal / independent "
                    "branches) -- a semantic mismatch, not a StepCheck finding.",
        },
        "note_data_flow": "SC1101 (data-flow / field provenance) has no BPMN control-flow expression; "
                          "it is StepCheck-unique by construction and is evaluated on the typed corpus "
                          "(eval/results-dataflow.json), not here.",
        "classes": classes_out,
        "control_verdicts": control_verdicts,
        "rows": rows,
    }
    with open(args.out, "w") as f:
        json.dump(report, f, indent=2)
    # compact console summary
    print("\n=== StepCheck vs Woflan vs BPMN Analyzer 2.0 vs BProVe (per-class recall) ===")
    print(f"{'class':30} {'n':>3} {'StepCheck':>14} {'Woflan':>14} {'BPMN-Anlz':>14} {'BProVe':>14}")
    for k, c in classes_out.items():
        sc = f"{c['stepcheck_recall']}" if c['applicable'] else "-"
        wf = f"{c['woflan_recall']}" if c['applicable'] else "-"
        ba = f"{c['bpmn_analyzer_recall']}" if (c['applicable'] and BPMN_ANALYZER) else "-"
        bp = f"{c['bprove_recall']}" if (c['applicable'] and bprove_configured()) else "-"
        print(f"{c['label']:30} {c['applicable']:>3} {sc:>14} {wf:>14} {ba:>14} {bp:>14}")
    cb = report["clean_baseline"]
    print(f"\nclean over-report -- Woflan: {cb['woflan_unsound']}/{cb['workflows']} = {cb['woflan_overreport_rate']}"
          + (f" | BPMN-Analyzer: {cb['bpmn_analyzer_unsound']}/{cb['workflows']} = {cb['bpmn_analyzer_overreport_rate']}" if BPMN_ANALYZER else "")
          + (f" | BProVe: {cb['bprove_unsound']}/{cb['workflows']} = {cb['bprove_overreport_rate']}" if bprove_configured() else ""))
    print(f"-> {os.path.relpath(args.out, ROOT)}")

if __name__ == "__main__":
    main()
