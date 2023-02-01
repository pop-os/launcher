debug := '0'
vendor := '0'

target := if debug == '1' { 'debug' } else { 'release' }
vendor_args := if vendor == '1' { '--frozen --offline' } else { '' }
debug_args := if debug == '1' { '' } else { '--release' }
cargo_args := vendor_args + ' ' + debug_args

plugins := 'calc desktop_entries files find pop_shell pulse recent scripts terminal web cosmic_toplevel'

ID := 'pop-launcher'

rootdir := ''

base_dir := if rootdir == '' {
    env_var('HOME') + '/.local/'
} else {
    rootdir + '/usr/'
}

lib_dir := if rootdir == '' {
    base_dir + 'share/'
} else {
    base_dir + 'lib/'
}

bin_dir := base_dir + 'bin/'
bin_path := bin_dir + ID

launcher_dir := lib_dir + ID + '/'
scripts_dir := launcher_dir + 'scripts/'
plugin_dir := launcher_dir + 'plugins/'

version := '0.0.0'

# Compile pop-launcher
all: _extract_vendor
    cargo build -p pop-launcher-bin {{cargo_args}}

check:
    cargo check -p pop-launcher-bin {{cargo_args}}

# Remove Cargo build artifacts
clean:
    cargo clean

# Also remove .cargo and vendored dependencies
distclean:
    rm -rf .cargo vendor vendor.tar target

# Install everything
install: install_bin install_plugins install_scripts

# Install pop-launcher binary
install_bin:
    install -Dm0755 target/{{target}}/pop-launcher-bin {{bin_path}}

# Install pop-launcher plugins
install_plugins:
    #!/usr/bin/env sh
    set -ex
    for plugin in {{plugins}}; do
        dest={{plugin_dir}}${plugin}
        mkdir -p ${dest}
        install -Dm0644 plugins/src/${plugin}/*.ron ${dest}
        ln -sf {{bin_path}} {{plugin_dir}}${plugin}/$(echo ${plugin} | sed 's/_/-/')
    done

# Install pop-launcher scripts
install_scripts:
    #!/usr/bin/env sh
    set -ex
    mkdir -p {{scripts_dir}}
    for script in {{justfile_directory()}}/scripts/*; do
        cp -r ${script} {{scripts_dir}}
    done

# Uninstalls everything (requires same arguments as given to install)
uninstall:
    rm {{bin_path}}
    rm -rf {{launcher_dir}}

# Vendor Cargo dependencies locally
vendor:
    mkdir -p .cargo
    cargo vendor --sync bin/Cargo.toml \
        --sync plugins/Cargo.toml \
        --sync service/Cargo.toml \
        | head -n -1 > .cargo/config
    echo 'directory = "vendor"' >> .cargo/config
    tar pcf vendor.tar vendor
    rm -rf vendor

# Extracts vendored dependencies if vendor=1
_extract_vendor:
    #!/usr/bin/env sh
    if test {{vendor}} = 1; then
        rm -rf vendor
        tar pxf vendor.tar
    fi
