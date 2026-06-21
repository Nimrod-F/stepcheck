# Regenerate paper/figures/eval-figure.pdf from corpus/manifest.json and the
# (human-gold) eval/inference_accuracy.json. Self-contained version of figures.ipynb.
import os, json, numpy as np, matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from collections import Counter

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
manifest = json.load(open(os.path.join(ROOT, "corpus/manifest.json")))
infer = json.load(open(os.path.join(ROOT, "eval/inference_accuracy.json")))

sizes = [w["totalStates"] for w in manifest]
type_tot = Counter()
for w in manifest:
    for t, n in w["types"].items():
        type_tot[t] += n

plt.rcParams.update({"font.size": 9, "font.family": "serif",
                     "axes.spines.top": False, "axes.spines.right": False})
C = {"blue": "#4878a8", "green": "#5a9367", "amber": "#d6a13a", "grey": "#9aa0a6"}
fig, ax = plt.subplots(1, 3, figsize=(11, 2.6))

# (a) workflow-size distribution
ax[0].hist(sizes, bins=range(1, max(sizes) + 3, 2), color=C["blue"], edgecolor="white")
ax[0].axvline(np.median(sizes), color=C["amber"], ls="--", lw=1.5, label=f"median {int(np.median(sizes))}")
ax[0].set_xlabel("states per workflow"); ax[0].set_ylabel("workflows")
ax[0].set_title("(a) Corpus size distribution"); ax[0].legend(frameon=False, fontsize=8)

# (b) state-type usage
order = ["Task", "Choice", "Pass", "Wait", "Fail", "Map", "Succeed", "Parallel"]
vals = [type_tot.get(t, 0) for t in order]
ax[1].barh(order[::-1], vals[::-1], color=C["green"], edgecolor="white")
for y, v in enumerate(vals[::-1]):
    ax[1].text(v + 8, y, str(v), va="center", fontsize=8)
ax[1].set_xlabel("state count"); ax[1].set_xlim(0, max(vals) * 1.18)
ax[1].set_title("(b) State-type usage")

# (c) inference accuracy vs coverage (human gold)
labels = ["Idempotency", "Persistence"]
acc = [infer["idempotency"]["accuracy_on_predicted"], infer["persistence"]["accuracy_on_predicted"]]
cov = [infer["idempotency"]["coverage"], infer["persistence"]["coverage"]]
x = np.arange(2); w = 0.36
b1 = ax[2].bar(x - w / 2, acc, w, color=C["blue"], label="accuracy")
b2 = ax[2].bar(x + w / 2, cov, w, color=C["grey"], label="coverage")
for b in list(b1) + list(b2):
    ax[2].text(b.get_x() + b.get_width() / 2, b.get_height() + 1.5, f"{b.get_height():.0f}", ha="center", fontsize=8)
ax[2].set_xticks(x); ax[2].set_xticklabels(labels); ax[2].set_ylim(0, 109); ax[2].set_ylabel("%")
ax[2].set_title("(c) Inference vs human gold"); ax[2].legend(frameon=False, fontsize=8, loc="lower center", ncol=2)

plt.tight_layout()
out = os.path.join(ROOT, "paper/figures/eval-figure.pdf")
plt.savefig(out, bbox_inches="tight")
plt.savefig(out.replace(".pdf", ".png"), dpi=150, bbox_inches="tight")
print("saved", out, "| idem", acc[0], "pers", acc[1], "cov", cov[0])
