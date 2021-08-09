# Pop Launcher

Modular IPC-based desktop launcher service, written in Rust. Desktop launchers may interface with this service via spawning the pop-launcher process and communicating to it via JSON IPC over the stdin and stdout pipes. The launcher service will also spawn plugins found in plugin directories on demand, based on the queries sent to the service.

Using IPC enables each plugin to isolate their data from other plugin processes and frontends that are interacting with them. If a plugin crashes, the launcher will continue functioning normally, gracefully cleaning up after the crashed process. Frontends and plugins may also be written in any language. The pop-launcher will do its part to schedule the execution of these plugins in parallel, on demand.

## Plugin Directories

- User-local plugins: `~/.local/share/pop-shell/plugins`
- System-wide install for system administrators: `/etc/pop-shell/plugins`
- Distribution packaging: `/usr/lib/pop-shell/plugins`

## Script Directories

- User-local plugins: `~/.local/share/pop-shell/scripts`
- System-wide install for system administrators: `/etc/pop-shell/scripts`
- Distribution packaging: `/usr/lib/pop-shell/scripts`

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
    /// Perform a tab completion from the selected item
    Complete(Indice),
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
- `{ "Complete": number }`
- `"Exit"`
- `"Interrupt"`
- `{ "Quit": number }`
- `{ "Search": string }`

### PluginResponse

If you are writing a plugin, you should send these events to your stdout.

```rust
pub enum PluginResponse {
    /// Append a new search item to the launcher
    Append(SearchMeta),
    /// Clear all results in the launcher list
    Clear,
    /// Close the launcher
    Close,
    // Notifies that a .desktop entry should be launched by the frontend
    DesktopEntry(PathBuf),
    /// Update the text in the launcher
    Fill(String),
    /// Indicoates that a plugin is finished with its queries
    Finished,
}
```

#### JSON Equivalent

- `{ "Append": SearchMeta }`,
- `"Clear"`,
- `"Close"`,
- `{ "DesktopEntry": string }`
- `{ "Fill": string }`
- `"Finished"`

Where `SearchMeta` is:

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

And `IconSource` is either:

- `{ "Name": string }`, where the name is a system icon, or an icon referred to by path
- `{ "Mime": string }`, where the mime is a mime essence string, to display file-based icons

### Response

Those implementing frontends should listen for these events:

```rust
pub enum Response {
    // An operation was performed and the frontend may choose to exit its process.
    Close,
    // Notifies that a .desktop entry should be launched by the frontend
    DesktopEntry(PathBuf),
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