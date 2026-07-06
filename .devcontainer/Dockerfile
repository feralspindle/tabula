# agent sandbox for tabula: node 20 + rust (with the wasm32 component
# toolchain) + psql + coding agents (claude, codex, opencode), non-root, with
# an egress firewall initialized at container start (see init-firewall.sh).
# deliberately absent: SSH keys, supabase CLI auth, docker socket, prod creds.

FROM node:20-bookworm

RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential pkg-config libssl-dev \
        git curl ca-certificates \
        iptables ipset dnsutils jq \
        sudo postgresql-client \
        less ripgrep \
    && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg \
        -o /usr/share/keyrings/githubcli-archive-keyring.gpg \
    && echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
        > /etc/apt/sources.list.d/github-cli.list \
    && apt-get update && apt-get install -y --no-install-recommends gh \
    && rm -rf /var/lib/apt/lists/*

# wasm-tools: prebuilt release binary (plugins/build.sh needs it)
RUN set -eux; \
    case "$(dpkg --print-architecture)" in \
        amd64) wt_arch=x86_64 ;; \
        arm64) wt_arch=aarch64 ;; \
    esac; \
    url="$(curl -fsSL https://api.github.com/repos/bytecodealliance/wasm-tools/releases/latest \
        | jq -r --arg a "$wt_arch" '.assets[] | select(.name | endswith($a + "-linux.tar.gz")) | .browser_download_url')"; \
    curl -fsSL "$url" | tar -xz -C /usr/local/bin --strip-components=1 --wildcards '*/wasm-tools'; \
    wasm-tools --version

COPY init-firewall.sh post-create-root.sh /usr/local/bin/
RUN chmod 755 /usr/local/bin/init-firewall.sh /usr/local/bin/post-create-root.sh \
    && echo "node ALL=(root) NOPASSWD: /usr/local/bin/init-firewall.sh, /usr/local/bin/post-create-root.sh" > /etc/sudoers.d/sandbox \
    && chmod 440 /etc/sudoers.d/sandbox

USER node
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal \
    && /home/node/.cargo/bin/rustup target add wasm32-unknown-unknown

# agent CLIs live in a node-user-owned npm prefix so their auto-updaters can
# write; a root-owned /usr/local install breaks self-update for the sandbox user.
ENV NPM_CONFIG_PREFIX=/home/node/.npm-global
ENV PATH=/home/node/.npm-global/bin:/home/node/.cargo/bin:$PATH
RUN npm install -g @anthropic-ai/claude-code @openai/codex opencode-ai

WORKDIR /workspaces
