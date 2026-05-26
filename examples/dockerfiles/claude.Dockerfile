FROM mcr.microsoft.com/devcontainers/base:ubuntu

ARG USERNAME=rtm
# devcontainers/base:ubuntu reserves UID/GID 1000 for its `vscode` user.
ARG USER_UID=1001
ARG USER_GID=1001

RUN groupadd --gid "${USER_GID}" "${USERNAME}" \
    && useradd --uid "${USER_UID}" --gid "${USER_GID}" --create-home --shell /bin/bash "${USERNAME}" \
    && mkdir -p /workspace \
    && chown "${USERNAME}:${USERNAME}" /workspace

# hadolint ignore=DL3008
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates git bash curl nodejs npm \
    && rm -rf /var/lib/apt/lists/*

RUN npm install -g @anthropic-ai/claude-code \
    && npm cache clean --force

WORKDIR /workspace
USER ${USERNAME}

CMD ["claude"]
