# StepCheck artifact image (ICSOC 2026 Artifact Evaluation).
#
#   docker build -t stepcheck-artifact .
#   docker run --rm -it stepcheck-artifact            # shell in /artifact
#   docker run --rm stepcheck-artifact node eval/reproduce.js --tier 1
#
# The image holds the tool, the corpora, the evaluation scripts and every offline
# baseline, pinned to the versions used for the paper:
#   statelint 0.8.0 (j2119 0.4.0), asl-validator 4.0.0, pm4py 2.7.23.1 (Woflan),
#   BPMN Analyzer 2.0 (rust_bpmn_analyzer @ ad34ecf), BProVe/BPMNOS parser + Maude 3.5.1.
# AWS ValidateStateMachineDefinition and the live deployments need an AWS account and
# are not part of the image; their recorded outputs ship in eval/ and infra/.
# Maude is distributed for x86_64 only: on Apple silicon build with --platform linux/amd64.

FROM rust:1.96-trixie

ARG RBA_COMMIT=ad34ecfea0ab4f713c488be26bcb76cfa1fa1423
ARG BPMNOS_COMMIT=0b6a80a41121626d1e38d190627dbc986c9b7084
ARG MAUDE_VERSION=3.5.1

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      nodejs npm python3 python3-venv python3-dev ruby ruby-dev \
      default-jre-headless graphviz unzip git ca-certificates build-essential \
 && rm -rf /var/lib/apt/lists/*

# Schema validators (Table 1, validator columns).
RUN gem install --no-document j2119:0.4.0 statelint:0.8.0 \
 && npm install -g asl-validator@4.0.0

# Woflan via pm4py, in a venv (Debian marks the system Python as externally managed).
RUN python3 -m venv /opt/venv \
 && /opt/venv/bin/pip install --no-cache-dir pm4py==2.7.23.1
ENV PATH="/opt/venv/bin:${PATH}"

# BPMN Analyzer 2.0 CLI, built from source at the commit current when the baselines ran.
RUN git clone https://github.com/timKraeuter/rust_bpmn_analyzer /opt/src/rba \
 && git -C /opt/src/rba checkout --quiet ${RBA_COMMIT} \
 && cargo build --release --manifest-path /opt/src/rba/cli/Cargo.toml \
 && install -m 755 "$(find /opt/src/rba -path '*/release/rust_bpmn_analyzer_cli' -type f | head -1)" /usr/local/bin/ \
 && rm -rf /opt/src/rba

# BProVe/BPMNOS: BPMN->Maude parser and the Maude operational semantics (PROS Lab).
RUN git clone https://bitbucket.org/proslabteam/bpmnos.git /opt/src/bpmnos \
 && git -C /opt/src/bpmnos checkout --quiet ${BPMNOS_COMMIT} \
 && mkdir -p /opt/bpmnos \
 && cp -r "/opt/src/bpmnos/BPMNOS_PARSER_v(last)" /opt/bpmnos/parser \
 && cp -r "/opt/src/bpmnos/BPMNOS_v(last)" /opt/bpmnos/model \
 && rm -rf /opt/src/bpmnos
RUN curl -fsSL -o /tmp/maude.zip \
      "https://github.com/maude-lang/Maude/releases/download/Maude${MAUDE_VERSION}/Maude-${MAUDE_VERSION}-linux-x86_64.zip" \
 && unzip -q /tmp/maude.zip -d /opt/maude && rm /tmp/maude.zip \
 && chmod +x /opt/maude/maude \
 && printf '#!/bin/sh\nexport MAUDE_LIB=/opt/maude\nexec /opt/maude/maude "$@"\n' > /usr/local/bin/maude \
 && chmod +x /usr/local/bin/maude \
 && echo 'quit .' | maude -no-banner
ENV BPMN_ANALYZER=/usr/local/bin/rust_bpmn_analyzer_cli \
    BPROVE_PARSER=/opt/bpmnos/parser/BPMNOS_Parser.jar \
    BPROVE_MAUDE_MODEL=/opt/bpmnos/model/BPMNOS_MODEL_CHECKER.maude

# The artifact itself.
WORKDIR /artifact
COPY . .
RUN cargo build --release --locked --manifest-path stepcheck/Cargo.toml \
 && cargo test --release --locked --manifest-path stepcheck/Cargo.toml --no-run
ENV STEPCHECK_BIN=/artifact/stepcheck/target/release/stepcheck \
    PATH="/artifact/stepcheck/target/release:${PATH}"

CMD ["bash"]
