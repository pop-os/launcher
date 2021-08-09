#!/usr/bin/gjs

const { GLib, Gio } = imports.gi;

const STDIN = new Gio.DataInputStream({ base_stream: new Gio.UnixInputStream({ fd: 0 }) })
const STDOUT = new Gio.DataOutputStream({ base_stream: new Gio.UnixOutputStream({ fd: 1 }) })

class App {
    constructor() {
        this.last_query = ""
        this.shell_only = false
    }

    /** @param {string} input */
    query(input) {
        if (input.startsWith(':')) {
            this.shell_only = true
            this.last_query = input.substr(1).trim()
        } else {
            this.shell_only = false
            this.last_query = input.startsWith('t:')
                ? input.substr(2).trim()
                : input.substr(input.indexOf(" ") + 1).trim()
        }

        this.send({ "Append": {
            id: 0,
            name: this.last_query,
            description: "run command in terminal"
        }})

        this.send("Finished")
    }

    /** @param {number} _id */
    submit(_id) {
        try {
            let runner
            if (this.shell_only) {
                runner = ""
            } else {
                let path = GLib.find_program_in_path('x-terminal-emulator');
                let [terminal, splitter] = path ? [path, "-e"] : ["gnome-terminal", "--"];
                runner = `${terminal} ${splitter} `
            }

            GLib.spawn_command_line_async(`${runner}sh -c '${this.last_query}; echo "Press to exit"; read t'`);
        } catch (e) {
            log(`command launch error: ${e}`)
        }

        this.send("Close")
    }

    /** @param {Object} object */
    send(object) {
        try {
            STDOUT.write_bytes(new GLib.Bytes(JSON.stringify(object) + "\n"), null)
        } catch (e) {
            log(`failed to send response to Pop Shell: ${e}`)
        }
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
                app.submit(event.Activate)
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