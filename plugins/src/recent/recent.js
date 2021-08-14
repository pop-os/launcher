#!/usr/bin/gjs

const { GLib, Gio, Gtk } = imports.gi

const STDIN = new Gio.DataInputStream({ base_stream: new Gio.UnixInputStream({ fd: 0 }) })
const STDOUT = new Gio.DataOutputStream({ base_stream: new Gio.UnixOutputStream({ fd: 1 }) })

class App {
    constructor() {
        this.last_query = ""
        this.manager = Gtk.RecentManager.get_default()
        this.results = new Array()
    }

    /**
     * @returns {null | Array<RecentItem>}
     */
    items() {
        const recent_items = this.manager.get_items()
        log(`got items`)

        if (!recent_items) { return null }

        const items = new Array()

        for (const item of recent_items) {
            if (item.exists()) {
                items.push({
                    display_name: item.get_display_name(),
                    mime: item.get_mime_type(),
                    uri: item.get_uri()
                })
            }
        }

        return items
    }

    query(input) {
        input = input.substr(input.indexOf(" ") + 1).trim()

        try {
            const items = this.items()

            if (items) {
                const normalized = input.toLowerCase()

                this.results = items
                    .filter(item => item.display_name.toLowerCase().includes(normalized))
                    .sort((a, b) => a.display_name.localeCompare(b.display_name))
                    .slice(0, 7)

                log(`sorted`)

                let id = 0

                for (const item of this.results) {
                    this.send({ "Append": {
                        id,
                        name: item.display_name,
                        description: decodeURI(item.uri),
                        icon: { Mime: item.mime }
                    }})

                    id += 1
                }
            }
        } catch (why) {
            log(`query exception: ${why}`)
        }

        this.send("Finished")
    }

    submit(id) {
        const result = this.results[id]

        if (result) {
            try {
                GLib.spawn_command_line_async(`xdg-open '${result.uri}'`)
            } catch (e) {
                log(`xdg-open failed: ${e}`)
            }
        }

        this.send("Close")
    }

    send(object) {
        STDOUT.write_bytes(new GLib.Bytes(JSON.stringify(object) + "\n"), null)
        STDOUT.flush(null)
    }
}

function main() {
    /** @type {null | ByteArray} */
    let input_array

    /** @type {string} */
    let input_str

    /** @type {null | LauncherRequest} */
    let event

    let app = new App()

    mainloop:
    while (true) {
        try {
            [input_array,] = STDIN.read_line(null)
        } catch (e) {
            break
        }

        input_str = imports.byteArray.toString(input_array)
        if ((event = parse_event(input_str)) !== null) {
            if ("Search" in event) {
                app.query(event.Search)
            } else if ("Activate" in event) {
                app.submit(event.Activate);
            } else if (event === "Exit") {
                break mainloop
            }
        }
    }
}

/**
 * Parses an IPC event received from STDIN
 * @param {string} input
 * @returns {null | LauncherRequest}
 */
function parse_event(input) {
    try {
        return JSON.parse(input)
    } catch (e) {
        log(`Input not valid JSON`)
        return null
    }
}

main()
