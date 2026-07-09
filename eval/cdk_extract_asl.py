#!/usr/bin/env python3
"""Extract the real ASL from a CDK-synthesized CloudFormation template.

Finds every AWS::StepFunctions::StateMachine, pulls its DefinitionString, and
resolves the CloudFormation intrinsics that appear inside a !Sub-style definition
(Fn::Sub with an inline string + {Fn::GetAtt}/{Ref} substitutions) into literal
placeholder ARNs, producing a parseable *.asl.json.

  python extract_asl_from_synth.py <cdk.out dir> <out dir>
"""
import sys, os, json, glob, re

PSEUDO = {
    "AWS::Partition": "aws",
    "AWS::Region": "us-east-1",
    "AWS::AccountId": "000000000000",
    "AWS::URLSuffix": "amazonaws.com",
    "AWS::StackName": "Stack",
}

def resolve_ref(key):
    """Resolve a CloudFormation Ref/Sub key to a literal, keeping AWS pseudo
    parameters exact (so service-integration ARNs like arn:aws:states:::sns:publish
    survive) and mapping resource references to a stable placeholder."""
    if key in PSEUDO:
        return PSEUDO[key]
    base = key.split(".")[0]              # ${Foo.Arn} -> Foo
    return f"arn:aws:lambda:us-east-1:000000000000:function:{base}"

def resolve_sub(node):
    """Resolve an { 'Fn::Sub': ... } (string, or [string, {vars}]) to a literal string."""
    v = node["Fn::Sub"]
    s = v[0] if isinstance(v, list) else v
    return re.sub(r"\$\{([^}]+)\}", lambda m: resolve_ref(m.group(1)), s)

def to_text(defn):
    """A DefinitionString may be a plain string, an Fn::Sub, or a Fn::Join."""
    if isinstance(defn, str):
        return defn
    if isinstance(defn, dict):
        if "Fn::Sub" in defn:
            return resolve_sub(defn)
        if "Fn::Join" in defn:
            sep, parts = defn["Fn::Join"]
            out = []
            for p in parts:
                if isinstance(p, str):
                    out.append(p)
                elif isinstance(p, dict):
                    if "Ref" in p:
                        out.append(resolve_ref(p["Ref"]))
                    elif "Fn::GetAtt" in p:
                        ga = p["Fn::GetAtt"]
                        out.append(resolve_ref(ga[0] if isinstance(ga, list) else ga))
                    elif "Fn::Sub" in p:
                        out.append(resolve_sub(p))
                    else:
                        out.append("PLACEHOLDER")
            return sep.join(out)
    return None

def main():
    outdir = sys.argv[2]
    os.makedirs(outdir, exist_ok=True)
    n = 0
    for tmpl in glob.glob(os.path.join(sys.argv[1], "*.template.json")):
        try:
            data = json.load(open(tmpl))
        except Exception:
            continue
        for name, res in (data.get("Resources") or {}).items():
            if res.get("Type") != "AWS::StepFunctions::StateMachine":
                continue
            defn = res.get("Properties", {}).get("DefinitionString")
            if defn is None:
                continue
            text = to_text(defn)
            if not text:
                print(f"  {name}: DefinitionString not a resolvable string form"); continue
            try:
                asl = json.loads(text)
            except Exception as e:
                print(f"  {name}: definition not valid JSON after resolve: {e}"); continue
            out = os.path.join(outdir, f"{name}.asl.json")
            json.dump(asl, open(out, "w"), indent=2)
            print(f"  extracted {name}: {len(asl.get('States', {}))} states -> {out}")
            n += 1
    print(f"total state machines extracted: {n}")

if __name__ == "__main__":
    main()
