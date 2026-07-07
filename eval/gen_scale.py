#!/usr/bin/env python3
"""Generate synthetic ASL workflows to measure StepCheck's scaling.

Modes:
    python gen_scale.py <n_states> <out.json>
    python gen_scale.py chain <n_states> <out.json>
    python gen_scale.py parallel <branches> <states_per_branch> <out.json>
    python gen_scale.py map <depth> <out.json>

The legacy two-argument form is a linear chain. The `parallel` mode stresses
wide fan-out with many sub-machines, and `map` stresses nested Map traversal.
Every Task reads a field with a `.$` reference and writes a fresh ResultPath, so
the data-flow transfer functions still run on each generated work state.

Reproduces the scalability numbers in the evaluation: generate one file per
size into its own directory, then run `stepcheck eval <dir>` and read
`timing_us` (the in-process per-workflow pipeline time, the same measurement
used for the main timing result).
"""
import json
import sys


def task(read_path: str, result_path: str, next_name: str | None) -> dict:
    state = {
        "Type": "Task",
        "Resource": "arn:aws:states:::lambda:invoke",
        "Parameters": {"FunctionName": "fn", "Payload.$": read_path},
        "ResultPath": result_path,
    }
    if next_name is None:
        state["End"] = True
    else:
        state["Next"] = next_name
    return state


def gen_chain(n: int) -> dict:
    if n < 2:
        raise ValueError("chain mode needs at least 2 states")
    states = {}
    # seed field
    states["S0"] = {"Type": "Pass", "Result": {"f0": 1},
                    "ResultPath": "$.f0", "Next": "S1"}
    for i in range(1, n - 1):
        states[f"S{i}"] = task(f"$.f{i - 1}", f"$.f{i}", f"S{i + 1}")
    states[f"S{n - 1}"] = {"Type": "Succeed"}
    return {"Comment": f"synthetic-{n}", "StartAt": "S0", "States": states}


def gen_parallel(branches: int, states_per_branch: int) -> dict:
    if branches < 1 or states_per_branch < 1:
        raise ValueError("parallel mode needs branches >= 1 and states_per_branch >= 1")
    branch_defs = []
    for b in range(branches):
        states = {}
        for i in range(states_per_branch):
            name = f"B{b}S{i}"
            next_name = f"B{b}S{i + 1}" if i + 1 < states_per_branch else None
            read_path = "$.seed" if i == 0 else f"$.b{b}_{i - 1}"
            states[name] = task(read_path, f"$.b{b}_{i}", next_name)
        branch_defs.append({"StartAt": f"B{b}S0", "States": states})
    states = {
        "Seed": {"Type": "Pass", "Result": {"seed": 1}, "ResultPath": "$", "Next": "Fan"},
        "Fan": {"Type": "Parallel", "Branches": branch_defs, "ResultPath": "$.fan", "Next": "Done"},
        "Done": {"Type": "Succeed"},
    }
    return {"Comment": f"synthetic-parallel-{branches}x{states_per_branch}", "StartAt": "Seed", "States": states}


def nested_map_state(level: int, depth: int) -> dict:
    if level == depth:
        return task("$.item", f"$.out{level}", None)
    child = nested_map_state(level + 1, depth)
    iterator = {"StartAt": f"M{level + 1}", "States": {f"M{level + 1}": child}}
    return {
        "Type": "Map",
        "ItemsPath": "$.items",
        "MaxConcurrency": 40,
        "Iterator": iterator,
        "ResultPath": f"$.m{level}",
        "End": True,
    }


def gen_map(depth: int) -> dict:
    if depth < 1:
        raise ValueError("map mode needs depth >= 1")
    states = {
        "Seed": {"Type": "Pass", "Result": {"items": [{"item": 1}]}, "ResultPath": "$", "Next": "M0"},
        "M0": nested_map_state(0, depth - 1),
    }
    return {"Comment": f"synthetic-map-depth-{depth}", "StartAt": "Seed", "States": states}


def usage() -> None:
    print(__doc__.strip(), file=sys.stderr)
    sys.exit(2)


if __name__ == "__main__":
    if len(sys.argv) == 3:
        workflow = gen_chain(int(sys.argv[1]))
        out = sys.argv[2]
    elif len(sys.argv) == 4 and sys.argv[1] == "chain":
        workflow = gen_chain(int(sys.argv[2]))
        out = sys.argv[3]
    elif len(sys.argv) == 5 and sys.argv[1] == "parallel":
        workflow = gen_parallel(int(sys.argv[2]), int(sys.argv[3]))
        out = sys.argv[4]
    elif len(sys.argv) == 4 and sys.argv[1] == "map":
        workflow = gen_map(int(sys.argv[2]))
        out = sys.argv[3]
    else:
        usage()
    with open(out, "w") as fh:
        json.dump(workflow, fh)
