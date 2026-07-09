#!/usr/bin/env python3
"""Build-time gate metric for AWS Solutions CDK synth artifacts.

This mirrors eval/industrial/wse.py's practitioner-facing metric on the
CDK-synthesised AWS Solutions templates: with 0/1/5 injected defects, measure
StepCheck's confound-free build-fail catch-rate, the platform validator's silent
pass-through, and end-to-end gate latency.

Mutations are applied to the ASL emitted from each CDK template and then embedded
back into a copy of the same CloudFormation template as DefinitionString, so the
timed gate still runs on a deployment artifact rather than on hand-extracted ASL.

Outputs eval/aws-solutions-cdk-gate.json.
"""
import argparse
import collections
import glob
import json
import os
import statistics
import subprocess
import sys
import tempfile
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

DEFAULT_PATTERNS = [
    "instance-scheduler-on-aws__*.template.json",
    "automated-security-response-on-aws__*.template.json",
    "distributed-load-testing-on-aws__*.template.json",
    "account-assessment-for-aws-organizations__*.template.json",
]

KIND_ORDER = ["retry", "compensation", "temporal", "contract", "structural", "concurrency"]
LEVELS = [0, 1, 5]

ASLV = os.path.join(
    os.environ.get("APPDATA", ""),
    "npm",
    "node_modules",
    "asl-validator",
    "dist",
    "bin",
    "asl-validator.js",
)


def default_bin():
    env = os.environ.get("STEPCHECK_BIN")
    if env:
        return env
    release = os.path.join(ROOT, "stepcheck", "target", "release", "stepcheck.exe")
    debug = os.path.join(ROOT, "stepcheck", "target", "debug", "stepcheck.exe")
    return release if os.path.exists(release) else debug


def run_stepcheck(bin_path, args):
    return subprocess.run(
        [bin_path] + args,
        capture_output=True,
        text=True,
        timeout=120,
    )


def cdk_templates(corpus, patterns):
    out = []
    for pat in patterns:
        out.extend(glob.glob(os.path.join(corpus, pat)))
    return sorted(dict.fromkeys(out))


def topology(asl_path):
    wf = json.load(open(asl_path, encoding="utf-8"))
    states = wf["States"]

    def count(pred):
        return sum(1 for state in states.values() if pred(state))

    return {
        "states": len(states),
        "tasks": count(lambda s: s.get("Type") == "Task"),
        "retries": count(lambda s: s.get("Retry")),
        "catches": count(lambda s: s.get("Catch")),
        "parallel": count(lambda s: s.get("Type") == "Parallel"),
        "map": count(lambda s: s.get("Type") == "Map"),
        "choice": count(lambda s: s.get("Type") == "Choice"),
    }


def emit_asl(bin_path, template, out_path):
    proc = run_stepcheck(bin_path, ["emit", template, "--out", out_path])
    return proc.returncode == 0 and os.path.exists(out_path), proc.stderr.strip()


def mutate_once(bin_path, cur, kind, seed, out_path):
    before = open(cur, encoding="utf-8").read()
    proc = run_stepcheck(bin_path, ["mutate", cur, "--kind", kind, "--seed", str(seed), "--out", out_path])
    if proc.returncode != 0 or not os.path.exists(out_path):
        return False
    after = open(out_path, encoding="utf-8").read()
    return after != before


def mutate_chain(bin_path, seed_asl, target, tmp):
    cur = seed_asl
    applied = []
    attempt = 0
    max_attempts = max(24, target * len(KIND_ORDER) * 8)
    while len(applied) < target and attempt < max_attempts:
        seed = attempt // len(KIND_ORDER)
        kind = KIND_ORDER[attempt % len(KIND_ORDER)]
        nxt = os.path.join(tmp, f"mut-{len(applied)}-{attempt}.json")
        if mutate_once(bin_path, cur, kind, seed, nxt):
            cur = nxt
            applied.append(kind)
        attempt += 1
    return cur, applied


def state_machine_resources(template_obj):
    resources = template_obj.get("Resources", {})
    return [
        (logical_id, resource)
        for logical_id, resource in resources.items()
        if resource.get("Type") in {
            "AWS::StepFunctions::StateMachine",
            "AWS::Serverless::StateMachine",
        }
    ]


def embed_asl(template_path, asl_path, out_path):
    template_obj = json.load(open(template_path, encoding="utf-8"))
    machines = state_machine_resources(template_obj)
    if len(machines) != 1:
        raise RuntimeError(f"expected exactly one state machine in {template_path}, found {len(machines)}")
    _, resource = machines[0]
    props = resource.setdefault("Properties", {})
    asl_obj = json.load(open(asl_path, encoding="utf-8"))
    props["DefinitionString"] = json.dumps(asl_obj, separators=(",", ":"))
    props.pop("Definition", None)
    props.pop("DefinitionUri", None)
    json.dump(template_obj, open(out_path, "w", encoding="utf-8"), indent=2)


def diagnostic_counts(bin_path, path):
    proc = run_stepcheck(bin_path, ["check", "--json", "--infer", path])
    try:
        data = json.loads(proc.stdout)
    except Exception:
        return collections.Counter(), {"error": True, "stderr": proc.stderr.strip()}
    diags = data.get("diagnostics", [])
    return collections.Counter(d.get("code") for d in diags), {
        "errors": sum(1 for d in diags if d.get("severity") == "error"),
        "warnings": sum(1 for d in diags if d.get("severity") == "warning"),
        "codes": dict(collections.Counter(d.get("code") for d in diags)),
    }


def platform_accepts(asl_path):
    if not os.path.exists(ASLV):
        return None
    proc = subprocess.run(
        ["node", ASLV, "--json-path", asl_path],
        capture_output=True,
        text=True,
        timeout=60,
    )
    return proc.returncode == 0


def gate_latency(bin_path, template_path, runs):
    ts = []
    for _ in range(runs):
        t0 = time.perf_counter()
        run_stepcheck(bin_path, ["check", "--infer", "--deny-warnings", template_path])
        ts.append((time.perf_counter() - t0) * 1000.0)
    return ts


def pct(sorted_values, p):
    if not sorted_values:
        return None
    return round(sorted_values[min(len(sorted_values) - 1, int(p * len(sorted_values)))], 1)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--corpus", default=os.path.join(ROOT, "corpus", "aws-solutions"))
    parser.add_argument("--out", default=os.path.join(ROOT, "eval", "aws-solutions-cdk-gate.json"))
    parser.add_argument("--bin", default=default_bin())
    parser.add_argument("--runs", type=int, default=40)
    parser.add_argument("--pattern", action="append", dest="patterns", default=None)
    args = parser.parse_args()

    patterns = args.patterns or DEFAULT_PATTERNS
    files = cdk_templates(args.corpus, patterns)
    if not files:
        raise SystemExit("no CDK synth templates matched")
    if not os.path.exists(args.bin):
        raise SystemExit(f"stepcheck binary not found: {args.bin}")

    gate = {
        str(level): {"applied": 0, "caught": 0, "platform_silent_pass": 0, "platform_n": 0, "lat": []}
        for level in LEVELS
    }
    details = []

    with tempfile.TemporaryDirectory(prefix="aws-solutions-cdk-gate-") as tmp:
        for idx, template in enumerate(files):
            rel = os.path.relpath(template, ROOT).replace("\\", "/")
            seed_asl = os.path.join(tmp, f"seed-{idx}.json")
            ok, err = emit_asl(args.bin, template, seed_asl)
            if not ok:
                details.append({"file": rel, "error": err or "emit failed"})
                continue
            control_counts, clean = diagnostic_counts(args.bin, template)
            rec = {
                "file": rel,
                "name": os.path.splitext(os.path.basename(template))[0],
                "topology": topology(seed_asl),
                "clean": clean,
                "levels": {},
            }

            for level in LEVELS:
                if level == 0:
                    asl_path = seed_asl
                    timed_template = template
                    applied = []
                else:
                    asl_path, applied = mutate_chain(args.bin, seed_asl, level, tmp)
                    timed_template = os.path.join(tmp, f"template-{idx}-{level}.json")
                    embed_asl(template, asl_path, timed_template)

                g = gate[str(level)]
                g["lat"].extend(gate_latency(args.bin, timed_template, args.runs))
                mutant_counts, _ = diagnostic_counts(args.bin, timed_template)
                caught = any(mutant_counts[code] > control_counts.get(code, 0) for code in mutant_counts)
                accepts = platform_accepts(asl_path)
                clean_accepts = rec.get("platform_clean_accepts")
                if level == 0:
                    rec["platform_clean_accepts"] = accepts
                    clean_accepts = accepts
                injectable = (level == 0) or (len(applied) == level)
                if injectable:
                    if level > 0:
                        g["applied"] += 1
                        g["caught"] += int(caught)
                    if accepts is not None and (level == 0 or clean_accepts is True):
                        g["platform_n"] += 1
                        g["platform_silent_pass"] += int(accepts)

                rec["levels"][str(level)] = {
                    "injected": len(applied),
                    "applied_kinds": applied,
                    "caught": caught if level else None,
                    "platform_accepts": accepts,
                }
            details.append(rec)
            print(
                f"  {rec['name'][:52]:52} states={rec['topology']['states']:>2} "
                f"clean={rec['clean'].get('errors', 0)}e/{rec['clean'].get('warnings', 0)}w",
                file=sys.stderr,
            )

    for level in LEVELS:
        g = gate[str(level)]
        ts = sorted(g.pop("lat"))
        g["gate_latency_ms"] = {
            "p50": round(statistics.median(ts), 1) if ts else None,
            "p95": pct(ts, 0.95),
            "p99": pct(ts, 0.99),
        }
        g["catch_rate"] = round(g["caught"] / g["applied"], 3) if g["applied"] else None
        g["platform_silent_pass_rate"] = (
            round(g["platform_silent_pass"] / g["platform_n"], 3) if g["platform_n"] else None
        )

    report = {
        "generated_by": "eval/industrial/aws_solutions_cdk_gate.py",
        "stepcheck_bin": os.path.relpath(args.bin, ROOT).replace("\\", "/"),
        "platform_validator": "asl-validator" if os.path.exists(ASLV) else "unavailable",
        "corpus": os.path.relpath(args.corpus, ROOT).replace("\\", "/"),
        "population": "AWS Solutions Library CDK synth templates",
        "patterns": patterns,
        "templates": len(details),
        "build_time_gate": {
            "metric": "at 0/1/5 defects injected after CDK synth: StepCheck gate catch-rate is a new finding on the mutant template vs the original template; platform silent-pass means asl-validator accepts the same emitted ASL definition. For mutant silent-pass, the denominator excludes templates the platform validator already rejects clean.",
            "by_injected_defects": gate,
        },
        "templates_detail": details,
    }
    json.dump(report, open(args.out, "w", encoding="utf-8"), indent=2)

    print("\n=== AWS Solutions CDK build-time gate ===")
    print(f"{'defects':>7} {'gate catch':>12} {'platform silent-pass':>22} {'gate p50/p95/p99 ms':>22}")
    for level in LEVELS:
        g = gate[str(level)]
        lat = g["gate_latency_ms"]
        print(
            f"{level:>7} {str(g['catch_rate']):>12} {str(g['platform_silent_pass_rate']):>22} "
            f"{str(lat['p50'])+'/'+str(lat['p95'])+'/'+str(lat['p99']):>22}"
        )
    print("-> " + os.path.relpath(args.out, ROOT))


if __name__ == "__main__":
    main()