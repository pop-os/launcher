#!/usr/bin/gjs

const { GLib, Gio } = imports.gi;

/** The directory that this script is executed from. */
const SCRIPT_DIR = GLib.path_get_dirname(new Error().stack.split(':')[0].slice(1));

/** Add our directory so we can import modules from it. */
imports.searchPath.push(SCRIPT_DIR)

const math = imports.math.math;
math.config({number: 'BigNumber' });

const STDIN = new Gio.DataInputStream({ base_stream: new Gio.UnixInputStream({ fd: 0 }) })
const STDOUT = new Gio.DataOutputStream({ base_stream: new Gio.UnixOutputStream({ fd: 1 }) })

class App {
    constructor() {
        this.last_query = ""
        this.last_value = ""
    }

    search(input) {
        this.last_query = input.substr(1)

        try {
            this.last_value = math.evaluate(this.last_query).toString()
        } catch (e) {
            this.last_value = this.last_query + ` x = ?`
        }

        this.send({ "Append": {
            id: 0,
            name: this.last_value,
            description: '',
            icon: { Name: 'accessories-calculator' },
        }})

        this.send("Finished")
    }

    activate(_id) {
        this.send({ "Fill": '= ' + this.last_value })
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
                app.search(event.Search);
            } else if ("Activate" in event) {
                app.activate(event.Activate);
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