#!/usr/bin/gjs

const { GLib, Gio } = imports.gi;

const STDIN = new Gio.DataInputStream({ base_stream: new Gio.UnixInputStream({ fd: 0 }) })
const STDOUT = new Gio.DataOutputStream({ base_stream: new Gio.UnixOutputStream({ fd: 1 }) })

class App {
    constructor() {
        /** @type Array<Selection> */
        this.selections = new Array()

        /** @type string */
        this.parent = ""

        /** @type string */
        this.last_query = ""
    }

    /**
     * Performs tab completion based on the last-given search query.
     *
     * @param {number} id
     */
    complete(id) {
        let text

        const selected = this.selections[id]
        if (selected) {
            text = selection_path(this.parent, selected)
        } else {
            text = this.last_query
        }

        this.send({ "Fill": text })
    }

    /**
     * Queries the plugin for results from this input
     *
     * @param {string} input
     */
    search(input) {
        if (input.startsWith('~')) {
            input = GLib.get_home_dir() + input.substr(1)
        }

        this.last_query = input

        // Add `/` to query if the input is a directory
        this.last_query = (!input.endsWith('/') && Gio.file_new_for_path(input).query_file_type(0, null) === 2)
            ? input + '/'
            : input

        this.selections.splice(0)
        this.parent = GLib.path_get_dirname(this.last_query)

        /** @type string */
        let base = GLib.path_get_basename(this.last_query)

        const show_hidden = base.startsWith('.')

        if (this.parent.endsWith(base)) base = ""

        try {
            const dir = Gio.file_new_for_path(this.parent)
            if (dir.query_exists(null)) {
                const entries = dir.enumerate_children('standard::*', Gio.FileQueryInfoFlags.NONE, null);
                let entry;

                while ((entry = entries.next_file(null)) !== null) {
                    /** @type {string} */
                    const name = entry.get_name()

                    if (base.length !== 0 && name.toLowerCase().indexOf(base.toLowerCase()) === -1) {
                        continue
                    }

                    if (!show_hidden && name.startsWith('.')) continue

                    const content_type = entry.get_content_type()
                    const directory = entry.get_file_type() === 2

                    this.selections.push({
                        id: 0,
                        name,
                        description: GLib.format_size_for_display(entry.get_size()),
                        content_type,
                        directory
                    })

                    if (this.selections.length === 20) break
                }
            }

            const pattern_lower = this.last_query.toLowerCase()

            this.selections
                .sort((a, b) => {
                    const a_name = a.name.toLowerCase()
                    const b_name = b.name.toLowerCase()

                    const a_includes = a_name.includes(pattern_lower)
                    const b_includes = b_name.includes(pattern_lower)

                    return ((a_includes && b_includes) || (!a_includes && !b_includes)) ? (a_name > b_name ? 1 : 0) : a_includes ? -1 : b_includes ? 1 : 0;
                })

            let id = 0
            for (const v of this.selections) {
                v.id = id
                id += 1
            }
        } catch (e) {
            log(`QUERY ERROR: ${e} `)
        }

        for (const selection of this.selections) {
            this.send({ "Append": {
                id: selection.id,
                name: selection.name,
                description: selection.description,
                icon: { "Mime": selection.content_type }
            }})
        }

        this.send("Finished")
    }

    /**
     * Applies an option that the user selected
     *
     * @param {number} id
     */
    activate(id) {
        const selected = this.selections[id]

        if (selected) {
            const path = selection_path(this.parent, selected)
            try {
                GLib.spawn_command_line_async(`xdg-open '${path}'`)
            } catch (e) {
                log(`xdg-open failed: ${e} `)
            }
        }

        this.send("Close")
    }

    /**
     * Sends message back to Pop Shell
     *
     * @param {Object} object
     */
    send(object) {
        STDOUT.write_bytes(new GLib.Bytes(JSON.stringify(object) + "\n"), null)
    }
}

/**
 *
 * @param {string} parent
 * @param {Selection} selection
 * @returns {string}
 */
function selection_path(parent, selection) {
    let text = parent
        + (parent.endsWith("/") ? "" : "/")
        + selection.name

    if (selection.directory) text += "/"

    return text
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
                app.search(event.Search)
            } else if ("Activate" in event) {
                app.activate(event.Activate)
            } else if ("Complete" in event) {
                app.complete(event.Complete)
            } else if ("Exit" === event) {
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
