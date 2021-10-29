ARG VARIANT="buster"
FROM mcr.microsoft.com/vscode/devcontainers/rust:0-${VARIANT}

RUN apt-get update && export DEBIAN_FRONTEND=noninteractive
RUN apt-get install libgtk-3-dev libssl-dev -y

# generate completions
RUN su vscode -c 'mkdir ~/.zfunc'
RUN su vscode -c 'rustup completions zsh > ~/.zfunc/_rustup'
RUN su vscode -c 'rustup completions zsh cargo > ~/.zfunc/_cargo'

# load completions via .zshrc
RUN su vscode -c 'echo fpath+=~/.zfunc >> ~/.zshrc'
RUN su vscode -c 'echo autoload -U compinit >> ~/.zshrc'
RUN su vscode -c 'echo compinit -i >> ~/.zshrc'
