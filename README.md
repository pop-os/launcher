# Pop Launcher

![](https://img.shields.io/badge/rustc-1.51-orange)

Modular IPC-based desktop launcher service, written in Rust. Desktop launchers may interface with this service via spawning the pop-launcher process and communicating to it via JSON IPC over the stdin and stdout pipes. The launcher service will also spawn plugins found in plugin directories on demand, based on the queries sent to the service.

Using IPC enables each plugin to isolate their data from other plugin processes and frontends that are interacting with them. If a plugin crashes, the launcher will continue functioning normally, gracefully cleaning up after the crashed process. Frontends and plugins may also be written in any language. The pop-launcher will do its part to schedule the execution of these plugins in parallel, on demand.

## Installation

Requires the following dependencies:

- [Just](https://github.com/casey/just)
- [Rust/Cargo](https://www.rust-lang.org/)

And then must be used with a compatible pop-launcher frontend

- [pop-shell](https://github.com/pop-os/shell/)
- [cosmic-launcher](https://github.com/pop-os/cosmic-launcher)
- [onagre](https://github.com/oknozor/onagre)

```sh
just # Build
just install # Install locally
```

Packaging for a Linux distribution?

```sh
just vendor # Vendor
just vendor=1 # Build with vendored dependencies
just rootdir=$(DESTDIR) install # Install to custom root directory
```

Want to install specific plugins? Remove the plugins you don't want:

```sh
just plugins="calc desktop_entries files find pop_shell pulse recent scripts terminal web" install
```

## Plugin Directories

- User-local plugins: `~/.local/share/pop-launcher/plugins/{plugin}/`
- System-wide install for system administrators: `/etc/pop-launcher/plugins/{plugin}/`
- Distribution packaging: `/usr/lib/pop-launcher/plugins/{plugin}/`

## Plugin Config

A plugin's metadata is defined `pop-launcher/plugins/{plugin}/plugin.ron`.

```ron
(
    name: "PluginName",
    description: "Plugin Description: Example",
    bin: (
        path: "name-of-executable-in-plugin-folder",
    )
    icon: Name("icon-name-or-path"),
    // Optional
    query: (
        // Optional -- if we should isolate this plugin when the regex matches
        isolate: true,
        // Optional -- Plugin which searches on empty queries
        persistent: true,
        // Optional -- avoid sorting results from this plugin
        no_sort: true,
        // Optional -- pattern that a query must have to be sent to plugin
        regex: "pattern"
    )
)
```

## Script Directories

- User-local scripts: `~/.local/share/pop-launcher/scripts`
- System-wide install for system administrators: `/etc/pop-launcher/scripts`
- Distribution packaging: `/usr/lib/pop-launcher/scripts`

Example script
<details>
<pre>
#!/bin/sh
#
# name: Connect to VPN
# icon: network-vpn
# description: Start VPN
# keywords: vpn start connect

nmcli connection up "vpn-name"
</pre>
</details>

## JSON IPC

Whether implementing a frontend or a plugin, the JSON codec used by pop-launcher is line-based. Every line will contain a single JSON message That will be serialized or decoded as a `Request`, `PluginResponse`, or `Response`. These types can be referenced in [docs.rs](https://docs.rs/pop-launcher). IPC is based on standard input/output streams, so you should take care not to write logs to stdout.

### Frontend JSON IPC

The frontend will send `Request`s to the pop-launcher service through the stdin pipe. The stdout pipe will respond with `Response`s. It is ideal to design your frontend to accept responses asynchronously. Sending `Interrupt` or `Search` will cancel any active searches being performed, if the plugins that are still actively searching support cancellation.

### Plugin JSON IPC

Plugins will receive `Request`s from pop-launcher through their stdin pipe. They should respond with `PluginResponse` messages.

### Request

If you are writing a frontend, you are sending these events to the pop-launcher stdin pipe. If you are writing a plugin, the plugin will be receiving these events from its stdin.

```rust
pub enum Request {
    /// Activate on the selected item
    Activate(Indice),
    /// Activate a context item on an item.
    ActivateContext { id: Indice, context: Indice },
    /// Perform a tab completion from the selected item
    Complete(Indice),
    /// Request for any context options this result may have.
    Context(Indice),
    /// Request to end the service
    Exit,
    /// Requests to cancel any active searches
    Interrupt,
    /// Request to close the selected item
    Quit(Indice),
    /// Perform a search in our database
    Search(String),
}
```

#### JSON Equivalent

- `{ "Activate": number }`
- `{ "ActivateContext": { "id": number, "context": id }}`
- `{ "Complete": number }`
- `{ "Context": number }`
- `"Exit"`
- `"Interrupt"`
- `{ "Quit": number }`
- `{ "Search": string }`

### PluginResponse

If you are writing a plugin, you should send these events to your stdout.

```rust
pub enum PluginResponse {
    /// Append a new search item to the launcher
    Append(PluginSearchResult),
    /// Clear all results in the launcher list
    Clear,
    /// Close the launcher
    Close,
    // Additional options for launching a certain item
    Context {
        id: Indice,
        options: Vec<ContextOption>,
    },
    // Notifies that a .desktop entry should be launched by the frontend.
    DesktopEntry {
        path: PathBuf,
        gpu_preference: GpuPreference,
    },
    /// Update the text in the launcher
    Fill(String),
    /// Indicoates that a plugin is finished with its queries
    Finished,
}
```

#### JSON Equivalent

- `{ "Append": PluginSearchResult }`,
- `"Clear"`,
- `"Close"`,
- `{ "Context": { "id": number, "options": Array<ContextOption> }}`
- `{ "DesktopEntry": { "path": string, "gpu_preference": GpuPreference }}`
- `{ "Fill": string }`
- `"Finished"`

Where `PluginSearchResult` is:

```ts
{
    id: number,
    name: string,
    description: string,
    keywords?: Array<string>,
    icon?: IconSource,
    exec?: string,
    window?: [number, number],
}
```

`ContextOption` is:

```ts
{
    id: number,
    name: string
}
```

`GpuPreference` is:

```ts
"Default" | "NonDefault"
```

And `IconSource` is either:

- `{ "Name": string }`, where the name is a system icon, or an icon referred to by path
- `{ "Mime": string }`, where the mime is a mime essence string, to display file-based icons

### Response

Those implementing frontends should listen for these events:

```rust
pub enum Response {
    // An operation was performed and the frontend may choose to exit its process.
    Close,
    // Additional options for launching a certain item
    Context {
        id: Indice,
        options: Vec<ContextOption>,
    },
    // Notifies that a .desktop entry should be launched by the frontend.
    DesktopEntry {
        path: PathBuf,
        gpu_preference: GpuPreference,
    },
    // The frontend should clear its search results and display a new list
    Update(Vec<SearchResult>),
    // An item was selected that resulted in a need to autofill the launcher
    Fill(String),
}
```

#### JSON Equivalent

- `"Close"`
- `{ "DesktopEntry": string }`
- `{ "Update": Array<SearchResult>}`
- `{ "Fill": string }`

Where `SearchResult` is:

```ts
{
    id: number,
    name: string,
    description: string,
    icon?: IconSource,
    category_icon?: IconSource,
    window?: [number, number]
}
```
