# Regenerate paper/figures/eval-detection.pdf from eval/validator-panel.json.
# Two panels: (a) injected-defect recall, StepCheck vs the three reachable
# validators; (b) in-the-wild complementarity over the 193-workflow corpus.
import os, json, numpy as np, matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
panel = json.load(open(os.path.join(ROOT, "eval/validator-panel.json")))

plt.rcParams.update({"font.size": 9, "font.family": "serif",
                     "axes.spines.top": False, "axes.spines.right": False})
C = {"blue": "#4878a8", "green": "#5a9367", "amber": "#d6a13a", "grey": "#9aa0a6",
     "red": "#b5544a"}

fig, ax = plt.subplots(1, 2, figsize=(11, 3.0), gridspec_kw={"width_ratios": [1.75, 1]})

# ---- (a) injected-defect recall by class ------------------------------------
md = panel["mutation_detection"]
order = ["structural", "contract", "retry", "compensation", "concurrency", "temporal"]
short = ["Dangling\ntransition", "Broken\nbinding", "Unsafe\nretry",
         "Missing\ncompens.", "Concurrency\nrace", "Heartbeat/\ntimeout"]
tools = ["StepCheck", "statelint", "asl-validator", "AWS"]
keys  = [None, "statelint", "asl-validator", "aws"]
cols  = [C["blue"], C["green"], C["amber"], C["grey"]]

x = np.arange(len(order)); w = 0.20
for i, (tool, key, col) in enumerate(zip(tools, keys, cols)):
    if key is None:                       # StepCheck: 100% recall on every class
        vals = [100.0] * len(order)
    else:
        vals = [md[c][key]["recall_pct"] for c in order]
    off = (i - 1.5) * w
    bars = ax[0].bar(x + off, vals, w, color=col, label=tool, edgecolor="white", linewidth=0.4)
    for b, v in zip(bars, vals):
        if v == 0:
            ax[0].text(b.get_x() + b.get_width() / 2, 2, "0", ha="center", va="bottom",
                       fontsize=6.5, color=C["red"])
ax[0].set_xticks(x); ax[0].set_xticklabels(short, fontsize=7.5)
ax[0].set_ylim(0, 109); ax[0].set_ylabel("recall (%)")
ax[0].set_title("(a) Injected-defect recall (616 mutants)")
ax[0].legend(frameon=False, fontsize=7.5, ncol=4, loc="upper center",
             bbox_to_anchor=(0.5, -0.34), columnspacing=1.2, handlelength=1.2)
ax[0].axvspan(1.5, 5.5, color=C["red"], alpha=0.05)
ax[0].text(3.5, 103, "no schema validator expresses these classes",
           ha="center", fontsize=7, color=C["red"])

# ---- (b) in-the-wild complementarity ----------------------------------------
v = panel["in_the_wild"]["venn_vs_stepcheck"]
segs  = [("both", v["both"], C["green"]),
         ("StepCheck only", v["stepcheck_only"], C["blue"]),
         ("baseline only", v["baseline_only"], C["amber"]),
         ("neither", v["neither"], C["grey"])]
left = 0
for label, val, col in segs:
    ax[1].barh(0, val, left=left, color=col, edgecolor="white", label=f"{label} ({val})")
    ax[1].text(left + val / 2, 0, str(val), ha="center", va="center", fontsize=8,
               color="white" if col != C["amber"] else "black")
    left += val
ax[1].set_xlim(0, left); ax[1].set_ylim(-0.6, 0.6)
ax[1].set_yticks([]); ax[1].set_xlabel("workflows (of 193)")
ax[1].set_title("(b) In-the-wild flagged workflows")
ax[1].legend(frameon=False, fontsize=7, loc="upper center",
             bbox_to_anchor=(0.5, -0.35), ncol=2, columnspacing=1.0, handlelength=1.0)

plt.tight_layout()
out = os.path.join(ROOT, "paper/figures/eval-detection.pdf")
plt.savefig(out, bbox_inches="tight")
plt.savefig(out.replace(".pdf", ".png"), dpi=150, bbox_inches="tight")
print("saved", out)
