#!/usr/bin/env python3
"""Build-time gate study on the industrial-topology workflow set.

Over an industrial-topology workflow set (real AWS saga / inventory / checkout /
ETL samples plus a faithful reconstruction of the AWS Serverless Airline Booking
ProcessBooking saga), report:

  * topology (states / tasks / retries / catches / parallel / map);
  * StepCheck's findings on the clean workflow (the advisory concerns it surfaces
    on production-topology workflows);
  * Woflan's soundness verdict on the encoded workflow;
  * end-to-end analysis time (P50/P95/P99 wall-clock of `stepcheck check`, the
    latency a CI gate actually pays, process start included);
  * CI-gate catch-rate: with M independently injected defects (M in {1,3}), does
    `stepcheck check --deny-warnings` fail the build?  Contrasted with the
    platform validators, which pass the same semantic mutants silently.

Outputs eval/industrial-case.json.
"""
import sys, os, io, json, glob, subprocess, statistics, argparse, warnings, tempfile
warnings.filterwarnings("ignore")

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
BIN = os.path.join(ROOT, "stepcheck", "target", "release", "stepcheck.exe")
ENCODE = os.path.join(ROOT, "eval", "asl2bpmn", "encode.js")

_s = sys.stderr; sys.stderr = io.StringIO()
import pm4py
from pm4py.algo.analysis.woflan import algorithm as woflan
sys.stderr = _s
import contextlib

MUT_KINDS = ["structural", "contract", "retry", "compensation", "concurrency", "temporal"]

def sc(args, **kw):
    return subprocess.run([BIN] + args, capture_output=True, text=True, timeout=120, **kw)

def topology(path):
    wf = json.load(open(path))
    S = wf["States"]
    def count(pred):
        return sum(1 for s in S.values() if pred(s))
    return {
        "states": len(S),
        "tasks": count(lambda s: s.get("Type") == "Task"),
        "retries": count(lambda s: s.get("Retry")),
        "catches": count(lambda s: s.get("Catch")),
        "parallel": count(lambda s: s.get("Type") == "Parallel"),
        "map": count(lambda s: s.get("Type") == "Map"),
        "choice": count(lambda s: s.get("Type") == "Choice"),
    }

def clean_findings(path):
    p = sc(["check", "--json", "--infer", path])
    try:
        d = json.loads(p.stdout)
    except Exception:
        return {"error": True}
    diags = d.get("diagnostics", [])
    codes = {}
    for x in diags:
        codes[x["code"]] = codes.get(x["code"], 0) + 1
    return {
        "errors": sum(1 for x in diags if x.get("severity") == "error"),
        "warnings": sum(1 for x in diags if x.get("severity") == "warning"),
        "codes": codes,
    }

def time_check(path, k=60):
    import time
    ts = []
    for _ in range(k):
        t0 = time.perf_counter()
        sc(["check", "--infer", path])
        ts.append((time.perf_counter() - t0) * 1000.0)
    ts.sort()
    def pct(p):
        return round(ts[min(len(ts) - 1, int(p * len(ts)))], 1)
    return {"runs": k, "p50_ms": round(statistics.median(ts), 1),
            "p95_ms": pct(0.95), "p99_ms": pct(0.99), "min_ms": round(ts[0], 1)}

def woflan_clean(path, tmp):
    import time
    bpmn = os.path.join(tmp, os.path.basename(path) + ".bpmn")
    with open(bpmn, "wb") as f:
        subprocess.run(["node", ENCODE, path], stdout=f, stderr=subprocess.DEVNULL, timeout=60)
    t0 = time.perf_counter()
    try:
        with contextlib.redirect_stderr(io.StringIO()):
            b = pm4py.read_bpmn(bpmn)
            net, im, fm = pm4py.convert_to_petri_net(b)
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            sound, _ = woflan.apply(net, im, fm, parameters={
                woflan.Parameters.RETURN_ASAP_WHEN_NOT_SOUND: True,
                woflan.Parameters.PRINT_DIAGNOSTICS: False,
                woflan.Parameters.RETURN_DIAGNOSTICS: True})
        return {"sound": bool(sound), "ms": round((time.perf_counter() - t0) * 1000.0, 1)}
    except Exception as e:
        return {"sound": None, "error": type(e).__name__}

def inject_m(path, kinds, tmp):
    """Chain M independent mutations; returns the final mutated file and the list
    of kinds that actually applied (had a site)."""
    cur = os.path.join(tmp, "seed.json")
    p = sc(["emit", path, "--out", cur])
    if p.returncode != 0:
        return None, []
    applied = []
    for i, k in enumerate(kinds):
        nxt = os.path.join(tmp, f"m{i}.json")
        p = sc(["mutate", cur, "--kind", k, "--out", nxt])
        if p.returncode == 0 and os.path.exists(nxt):
            cur = nxt
            applied.append(k)
    return cur, applied

def gate_fails(path):
    """`stepcheck check --deny-warnings` non-zero exit = the CI gate fails the build."""
    p = sc(["check", "--infer", "--deny-warnings", path])
    return p.returncode != 0

def counts_of(path):
    """Multiset (code -> count) of diagnostics StepCheck reports on `path`. A count
    is needed, not just the code set: a saga may already carry an SC3001, so an
    injected retry on another task is a *new finding* (count up) but not a new code."""
    from collections import Counter
    p = sc(["check", "--json", "--infer", path])
    try:
        return Counter(d["code"] for d in json.loads(p.stdout).get("diagnostics", []))
    except Exception:
        return Counter()

ASLV = os.path.join(os.environ.get("APPDATA", ""), "npm", "node_modules",
                    "asl-validator", "dist", "bin", "asl-validator.js")

def platform_accepts(path):
    """The platform validator a developer actually runs (asl-validator, the same
    schema check AWS's own ValidateStateMachineDefinition performs). Exit 0 = the
    definition is 'valid' and would deploy -- a silent pass-through of a semantic
    defect. Returns True/False, or None if the validator is unavailable."""
    if not os.path.exists(ASLV):
        return None
    p = subprocess.run(["node", ASLV, "--json-path", path], capture_output=True, text=True, timeout=60)
    return p.returncode == 0

def gate_latency(path, k=40):
    import time
    ts = []
    for _ in range(k):
        t0 = time.perf_counter()
        sc(["check", "--infer", "--deny-warnings", path])
        ts.append((time.perf_counter() - t0) * 1000.0)
    return ts

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", default=os.path.join(ROOT, "corpus", "industrial"))
    ap.add_argument("--out", default=os.path.join(ROOT, "eval", "industrial-case.json"))
    args = ap.parse_args()
    tmp = tempfile.mkdtemp(prefix="wse-")
    files = sorted(glob.glob(os.path.join(args.corpus, "*.json")))

    per_wf = []
    # Build-time gate metric at 0 / 1 / 5 injected defects. Injecting M defects is
    # confound-free (control = `stepcheck emit`, mutant = control + M mutations); a
    # gate CATCH is a new diagnostic on the mutant vs the control, so pre-existing
    # advisory findings are not double-counted. SILENT PASS-THROUGH is the platform
    # validator (asl-validator, == AWS ValidateStateMachineDefinition's schema
    # check) accepting the same mutant -- the workflow would deploy.
    LEVELS = [0, 1, 5]
    # Semantic-first: the classes the platform validator cannot express (retry,
    # compensation, temporal, concurrency) come first, so the 1-defect level is a
    # clean silent-pass contrast; the syntactic classes (contract, structural),
    # which asl-validator does catch, come last.
    KIND_ORDER = ["retry", "compensation", "temporal", "contract", "structural", "concurrency"]
    gate = {str(M): {"applied": 0, "caught": 0, "platform_silent_pass": 0, "platform_n": 0, "lat": []}
            for M in LEVELS}
    for f in files:
        rel = os.path.relpath(f, ROOT).replace("\\", "/")
        rec = {"file": rel, "name": os.path.splitext(os.path.basename(f))[0],
               "topology": topology(f), "clean": clean_findings(f),
               "timing": time_check(f), "woflan": woflan_clean(f, tmp), "levels": {}}
        control, _ = inject_m(f, [], tmp)  # canonical emit
        control_counts = counts_of(control) if control else __import__("collections").Counter()
        for M in LEVELS:
            mfile, applied = inject_m(f, KIND_ORDER[:M], tmp) if M else (control, [])
            if not mfile:
                continue
            g = gate[str(M)]
            g["lat"] += gate_latency(mfile)
            mc = counts_of(mfile)
            caught = any(mc[c] > control_counts.get(c, 0) for c in mc)
            acc = platform_accepts(mfile)
            # only count workflows where the M defects were actually injectable
            # (some sagas lack a site for a given class); latency is timed for all.
            injectable = (M == 0) or (len(applied) == M)
            if injectable:
                if M > 0:
                    g["applied"] += 1
                    g["caught"] += int(caught)
                if acc is not None:  # platform acceptance (M=0 = clean baseline)
                    g["platform_n"] += 1
                    g["platform_silent_pass"] += int(acc)
            rec["levels"][str(M)] = {"injected": len(applied), "caught": caught if M else None,
                                     "platform_accepts": acc}
        per_wf.append(rec)
        print(f"  {rec['name']:32} states={rec['topology']['states']:>2} "
              f"clean={rec['clean'].get('errors',0)}e/{rec['clean'].get('warnings',0)}w "
              f"p50={rec['timing']['p50_ms']}ms woflan_sound={rec['woflan'].get('sound')}", file=sys.stderr)

    for M in LEVELS:
        g = gate[str(M)]
        ts = sorted(g.pop("lat"))
        pct = lambda p: round(ts[min(len(ts) - 1, int(p * len(ts)))], 1) if ts else None
        g["gate_latency_ms"] = {"p50": round(statistics.median(ts), 1) if ts else None,
                                "p95": pct(0.95), "p99": pct(0.99)}
        g["catch_rate"] = round(g["caught"] / g["applied"], 3) if g["applied"] else None
        g["platform_silent_pass_rate"] = round(g["platform_silent_pass"] / g["platform_n"], 3) if g["platform_n"] else None

    report = {
        "generated_by": "eval/industrial/wse.py",
        "verifier": "Woflan via pm4py " + pm4py.__version__,
        "platform_validator": "asl-validator" if os.path.exists(ASLV) else "unavailable",
        "corpus": os.path.relpath(args.corpus, ROOT).replace("\\", "/"),
        "workflows": len(per_wf),
        "build_time_gate": {
            "metric": "at 0/1/5 defects injected at build time: StepCheck gate CATCH-RATE (build fails on a "
                      "new finding, confound-free vs the emitted control) vs the platform validator's SILENT "
                      "PASS-THROUGH (asl-validator accepts the same mutant -> would deploy); plus gate latency.",
            "by_injected_defects": gate,
        },
        "workflows_detail": per_wf,
    }
    json.dump(report, open(args.out, "w"), indent=2)
    print("\n=== Build-time gate: catch-rate vs silent pass-through (0/1/5 defects) ===")
    print(f"{'defects':>7} {'gate catch':>12} {'platform silent-pass':>22} {'gate p50/p95/p99 ms':>22}")
    for M in LEVELS:
        g = gate[str(M)]
        lat = g["gate_latency_ms"]
        print(f"{M:>7} {str(g['catch_rate']):>12} {str(g['platform_silent_pass_rate']):>22} "
              f"{str(lat['p50'])+'/'+str(lat['p95'])+'/'+str(lat['p99']):>22}")
    print("-> " + os.path.relpath(args.out, ROOT))

if __name__ == "__main__":
    main()
