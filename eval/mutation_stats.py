#!/usr/bin/env python3
"""WS-A: exact statistics so no recall is reported as a bare "100%".

Reads the full-corpus StepCheck-vs-formal-verifier comparison and reports, per SC
class, the exact Clopper-Pearson 95% confidence interval on each tool's recall
(k/n). For a deterministic 100% (k = n) the exact lower bound is
(0.025)^(1/n) < 1 --- a proper, non-degenerate uncertainty statement that the
bootstrap interval [1,1] cannot express on small n. Also records the real-bug
evidence (curated fix-commit pairs + independent-repo wild findings) with honest
framing.

  python eval/mutation_stats.py   -> eval/mutation-stats.json
"""
import json, os, random
from scipy.stats import beta, fisher_exact

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

def clopper_pearson(k, n, alpha=0.05):
    if n == 0:
        return [None, None]
    lo = 0.0 if k == 0 else beta.ppf(alpha / 2, k, n - k + 1)
    hi = 1.0 if k == n else beta.ppf(1 - alpha / 2, k + 1, n - k)
    return [round(float(lo), 3), round(float(hi), 3)]

def load(p):
    return json.load(open(os.path.join(ROOT, p)))

cmp_full = load("eval/asl2bpmn-comparison-full.json")
native_full = load("eval/results-dataflow-result-shapes.json")

AGG_CLASSES = [
    ("structural", "Structural", "SC0002"),
    ("contract", "Contract", "SC1003"),
    ("dataflow", "Dataflow", "SC1101"),
    ("retry", "Retry", "SC3001"),
    ("compensation", "Compensation", "SC4001"),
    ("concurrency", "Concurrency", "SC5001"),
    ("temporal", "Temporal", "SC6003"),
]

def flags(k, n):
    return [1] * int(k) + [0] * max(0, int(n) - int(k))

def percentile(xs):
    if not xs:
        return [None, None]
    xs = sorted(xs)
    return [round(xs[int(0.025 * len(xs))], 3), round(xs[int(0.975 * len(xs))], 3)]

def prf(tp, fp, fn):
    precision = tp / (tp + fp) if tp + fp else 0.0
    recall = tp / (tp + fn) if tp + fn else 0.0
    f1 = 2 * precision * recall / (precision + recall) if precision + recall else 0.0
    return precision, recall, f1

def summarize_flags(class_flags, class_names=None, B=3000, seed=17):
    """Macro operating-point AP/F1 for deterministic binary tools.

    The tools do not rank findings, so AP is not ranked average precision. It is
    the macro operating-point precision over credited fresh detections; clean
    over-report is reported separately in clean_baseline_overreport.
    """
    if class_names is None:
        class_names = list(class_flags)
    per_class = {}
    for name in class_names:
        vals = class_flags[name]
        tp = sum(vals)
        fn = len(vals) - tp
        precision, recall, f1 = prf(tp, 0, fn)
        per_class[name] = {
            "applicable": len(vals),
            "detected": tp,
            "precision": round(precision, 3),
            "recall": round(recall, 3),
            "f1": round(f1, 3),
        }
    macro_ap = sum(v["precision"] for v in per_class.values()) / len(per_class)
    macro_f1 = sum(v["f1"] for v in per_class.values()) / len(per_class)
    rng = random.Random(seed)
    ap_samples, f1_samples = [], []
    for _ in range(B):
        ps, fs = [], []
        for name in class_names:
            vals = class_flags[name]
            if not vals:
                ps.append(0.0)
                fs.append(0.0)
                continue
            sample = [vals[rng.randrange(len(vals))] for _ in range(len(vals))]
            p, _, f = prf(sum(sample), 0, len(sample) - sum(sample))
            ps.append(p)
            fs.append(f)
        ap_samples.append(sum(ps) / len(ps))
        f1_samples.append(sum(fs) / len(fs))
    return {
        "macro_operating_point_ap": round(macro_ap, 3),
        "macro_operating_point_ap_ci95": percentile(ap_samples),
        "macro_f1": round(macro_f1, 3),
        "macro_f1_ci95": percentile(f1_samples),
        "per_class": per_class,
    }

def aggregate_metrics():
    native_by_kind = native_full["mutation_study"]
    class_flags = {tool: {} for tool in ["stepcheck", "woflan", "bpmn_analyzer", "bprove"]}
    fair_class = {}
    for key, native_key, code in AGG_CLASSES:
        native_row = native_by_kind[native_key]
        n_native = native_row["applicable"]
        class_flags["stepcheck"][key] = flags(native_row["detected"], n_native)
        formal_row = cmp_full["classes"].get(key)
        if formal_row:
            fair_class[key] = {
                "code": code,
                "bpmn_fair_class": bool(formal_row.get("control_flow_expressible")),
                "formal_applicable": formal_row["applicable"],
            }
            for tool, field in [("woflan", "woflan_detected"), ("bpmn_analyzer", "bpmn_analyzer_detected"), ("bprove", "bprove_detected")]:
                class_flags[tool][key] = flags(formal_row.get(field, 0) or 0, formal_row["applicable"])
        else:
            fair_class[key] = {
                "code": code,
                "bpmn_fair_class": False,
                "formal_applicable": 0,
                "reason": "No ASL data-flow field-provenance expression in the BPMN soundness encoding.",
            }
            for tool in ["woflan", "bpmn_analyzer", "bprove"]:
                class_flags[tool][key] = flags(0, n_native)
    return {
        "method": "Full 193-workflow aggregate over seven StepCheck mutation classes. AP is macro operating-point precision for deterministic binary tools, not ranked average precision; clean over-report is reported separately.",
        "tools": {
            tool: {
                "all_seven_classes": summarize_flags(class_flags[tool]),
                "strict_bpmn_soundness_subset": summarize_flags(class_flags[tool], ["structural"]),
            }
            for tool in class_flags
        },
        "fair_class": fair_class,
        "scope_interpretation": {
            "do_not_claim": "Do not phrase the result as a percentage-win comparison against BProVe.",
            "preferred_claim": "BProVe's soundness checking and StepCheck's ASL-native analyses are complementary: BProVe has no ASL data-flow model, so SC1101 is StepCheck versus out-of-scope, not a BProVe failure.",
            "stepcheck_native_only": ["SC1003", "SC1101", "SC3001", "SC6003"],
            "formal_soundness_overlap": ["SC0002"],
            "formal_structural_side_effects": ["SC4001", "SC5001"],
        },
    }

classes = {}
for k, c in cmp_full["classes"].items():
    n = c["applicable"]
    sc_k = c["stepcheck_detected"]
    wf_k = c["woflan_detected"]
    ba_k = c.get("bpmn_analyzer_detected")
    bp_k = c.get("bprove_detected")
    # Fisher exact test of StepCheck vs each formal verifier on the same n mutants.
    _, p_wf = fisher_exact([[sc_k, n - sc_k], [wf_k, n - wf_k]])
    entry = {
        "label": c["label"],
        "n": n,
        "stepcheck": {"detected": sc_k, "recall": c["stepcheck_recall"], "cp95": clopper_pearson(sc_k, n)},
        "woflan": {"detected": wf_k, "recall": c["woflan_recall"], "cp95": clopper_pearson(wf_k, n)},
        "fisher_exact_vs_woflan_p": float(p_wf),
    }
    if ba_k is not None:
        _, p_ba = fisher_exact([[sc_k, n - sc_k], [ba_k, n - ba_k]])
        entry["bpmn_analyzer"] = {"detected": ba_k, "recall": c["bpmn_analyzer_recall"], "cp95": clopper_pearson(ba_k, n)}
        entry["fisher_exact_vs_bpmn_analyzer_p"] = float(p_ba)
    if bp_k is not None:
        _, p_bp = fisher_exact([[sc_k, n - sc_k], [bp_k, n - bp_k]])
        entry["bprove"] = {"detected": bp_k, "recall": c["bprove_recall"], "cp95": clopper_pearson(bp_k, n)}
        entry["fisher_exact_vs_bprove_p"] = float(p_bp)
    classes[k] = entry

# Real-bug evidence (honest framing: existence + wild).
wild = load("eval/results-wild.json")["code_totals"]
real_bugs = {
    "curated_fix_commit_pairs": 7,
    "curated_note": "before/after pairs mined from public fix commits; each confirmed by the fix.",
    "wild_sound_tier_findings": {
        "SC1101": wild.get("SC1101", 0),
        "SC5001": wild.get("SC5001", 0),
        "corpus": "corpus/wild-external: 95 ASL workflows from 16 independent repos (outside aws-samples)",
    },
    "issue_tracker_user_reported": {
        "count": 3,
        "class": "SC1101",
        "examples": ["aws-samples/serverless-coffee-workshop#56 (official AWS)", "scttfrdmn/campus-compute#32", "dataPlor/turbofan#1"],
        "detail": "eval/issue-tracker-defects.json",
        "note": "user-filed issues whose quoted runtime error is exactly what StepCheck's sound tier prevents (not author-selected diffs); one from an official AWS workshop.",
    },
    "framing": "7 curated fix-commit defects + 3 user-reported issue-tracker defects (SC1101, one official AWS) are an "
               "existence result; the wild corpus adds label-free sound-tier findings (SC1101, SC5001) "
               "on independent repositories. We do not claim a population prevalence.",
}

def load_opt(p):
    try:
        return load(p)
    except Exception:
        return None

# --- Hard-mutant study (W1: operator-circularity rebuttal) -------------------
# Each class's boundary variant is a genuine defect placed just past the
# analysis's ⊤ / coverage boundary. We report StepCheck's recall from the native
# typed-tier study (count-based fresh, family-credited), the formal verifiers from
# the round-trip comparison, and the schema validators from the panel, all with
# exact Clopper-Pearson CIs. A recall < 1 here is a *characterized* boundary, not
# a soundness defect: at ⊤ the analysis correctly stays silent (no false alarm).
def hard_mutant_metrics():
    native = load_opt("eval/results-hard-mutants.json")
    if not native:
        return None
    formal = load_opt("eval/asl2bpmn-comparison-hard.json")
    schema = load_opt("eval/validator-panel-hard.json")
    study = native["mutation_study_hard"]
    conf = native.get("hard_confusion_matrix", {})
    # (native kind, code, formal/schema key, boundary description)
    HARD_CLASSES = [
        ("Structural", "SC0002", "structural", "exact decision procedure (reachability; recurses into nested scopes)"),
        ("Contract", "SC1003", "contract", "generalization control: the break moves from Parameters into a ResultSelector, which SC1003 reads like every other payload template"),
        ("Retry", "SC3001", "retry", "inference-tier boundary (non-idempotence unprovable once name/FunctionName are generic)"),
        ("Compensation", "SC4001", "compensation", "family generalization (caught by the effect-aware sibling SC4010, not the presence-only SC4001)"),
        ("Concurrency", "SC5001", "concurrency", "sound ⊤-lift (dynamically-named shared resource)"),
        ("Temporal", "SC6003", "temporal", "sound ⊤-lift (heartbeat/timeout supplied via reference paths)"),
        ("Dataflow", "SC1101", None, "sound ⊤-lift (filter-expression InputPath re-roots the document to ⊤)"),
    ]
    out = {}
    for kind, code, key, boundary in HARD_CLASSES:
        row = study.get(kind)
        if not row:
            continue
        n = row["applicable"]
        de = row["detected_expected_code"]
        df = row["detected_family"]
        entry = {
            "code": code,
            "n": n,
            "boundary": boundary,
            "stepcheck_expected_code": {"detected": de, "recall": round(de / n, 3) if n else None, "cp95": clopper_pearson(de, n)},
            "stepcheck_family": {"detected": df, "recall": round(df / n, 3) if n else None, "cp95": clopper_pearson(df, n)},
            "codes_fired": conf.get(kind, {}),
        }
        if formal and key and key in formal.get("classes", {}):
            fc = formal["classes"][key]
            nf = fc["applicable"]
            for tool, field in [("woflan", "woflan_detected"), ("bpmn_analyzer", "bpmn_analyzer_detected"), ("bprove", "bprove_detected")]:
                k = fc.get(field)
                if k is None:
                    continue
                entry[tool] = {"detected": k, "recall": round(k / nf, 3) if nf else None, "cp95": clopper_pearson(k, nf)}
        if schema and key and key in schema.get("mutation_detection", {}):
            sc = schema["mutation_detection"][key]
            ns = sc["applicable"]
            for tool in ["statelint", "asl-validator", "aws"]:
                d = sc.get(tool, {}).get("detected")
                if d is None:
                    continue
                entry[tool.replace("-", "_")] = {"detected": d, "recall": round(d / ns, 3) if ns else None, "cp95": clopper_pearson(d, ns)}
        out[kind] = entry
    return {
        "method": "Boundary variant per class (stepcheck mutate --hard). StepCheck recall from the native typed-tier study "
                  "(count-based fresh, family-credited); formal verifiers from the round-trip comparison; schema validators "
                  "from the panel. Exact Clopper-Pearson 95% CIs throughout.",
        "interpretation": "recall < 1 is a measured completeness boundary, not a soundness violation: at ⊤ the sound analyses "
                          "decline to report what they cannot prove (no false alarm). The abstraction-gap classes (data-flow, "
                          "concurrency, temporal) drop to 0 at their boundary; compensation is caught by a sibling code rather "
                          "than the operator's expected code; structural reachability stays exact; retry degrades gracefully at "
                          "the inference tier; the contract break is caught wherever it hides, because the SC1003 scan covers every payload "
                          "template (Parameters, ResultSelector, ItemSelector, Assign) and Choice guards.",
        "classes": out,
    }

report = {
    "generated_by": "eval/mutation_stats.py",
    "method": "Clopper-Pearson exact 95% CI on per-class recall k/n over the full 193-workflow corpus; Fisher exact vs each formal verifier.",
    "hard_mutants": hard_mutant_metrics(),
    "verifier_comparison": cmp_full["verifier"],
    "verifier_comparison2": cmp_full.get("verifier2"),
    "verifier_comparison3": cmp_full.get("verifier3"),
    "classes": classes,
    "aggregate_metrics": aggregate_metrics(),
    "clean_baseline_overreport": cmp_full["clean_baseline"],
    "real_bugs": real_bugs,
}
json.dump(report, open(os.path.join(ROOT, "eval", "mutation-stats.json"), "w"), indent=2)

print("=== WS-A exact per-class recall CIs (Clopper-Pearson 95%) ===")
print(f"{'class':32} {'n':>4} {'StepCheck k/n (CI)':>26} {'Woflan k/n (CI)':>24} {'BPMN-Anlz k/n (CI)':>24} {'BProVe k/n (CI)':>24}")
for k, c in classes.items():
    s, w = c["stepcheck"], c["woflan"]
    scol = f"{s['detected']}/{c['n']} {s['cp95']}"
    wcol = f"{w['detected']}/{c['n']} {w['cp95']}"
    ba = c.get("bpmn_analyzer")
    bp = c.get("bprove")
    bacol = f"{ba['detected']}/{c['n']} {ba['cp95']}" if ba else "-"
    bpcol = f"{bp['detected']}/{c['n']} {bp['cp95']}" if bp else "-"
    print(f"{c['label']:32} {c['n']:>4} {scol:>26} {wcol:>24} {bacol:>24} {bpcol:>24}")
print(f"\nreal bugs: 7 curated + wild SC1101x{real_bugs['wild_sound_tier_findings']['SC1101']}, "
      f"SC5001x{real_bugs['wild_sound_tier_findings']['SC5001']} (independent repos)")
print("aggregate AP/F1: full 193-workflow seven-class metrics added to eval/mutation-stats.json")
print("-> eval/mutation-stats.json")
