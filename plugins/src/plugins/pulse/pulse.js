#!/usr/bin/gjs

const { GLib, Gio } = imports.gi;

const STDIN = new Gio.DataInputStream({ base_stream: new Gio.UnixInputStream({ fd: 0 }) })
const STDOUT = new Gio.DataOutputStream({ base_stream: new Gio.UnixOutputStream({ fd: 1 }) })

/**
 * @typedef {Object} Sink
 * @property {number} id
 * @property {string} description
 */

/**
 * @returns {null | Array<Sink>}
 */
function pactl_sinks() {
    try {
        const resp = async_process(["pactl", "list", "sinks"])
        if (!resp) return null

        const { proc, stdout } = resp

        let sinks = new Array()
        let sink = {}

        while (true) {
            const [bytes] = stdout.read_line(null)
            if (bytes === null) break

            const line = imports.byteArray.toString(bytes)
            if (line.startsWith("Sink")) {
                sink.id = line.substr(6)
            } else if (line.includes("Description:")) {
                sink.description = line.split(' ').slice(1).join(' ')
                sinks.push({ ...sink })
            }
        }

        return sinks
    } catch (e) {
        log(`error: ${e}`)
        return null
    }
}

class App {
    constructor() {
        this.last_query = ""
        this.shell_only = false

        this.default_selections = [
            {
                id: 0,
                name: "Toggle Mute",
                description: "Silence and unsilence the default audio sink",
            },

            {
                id: 1,
                name: "Volume Up",
                description: "Raise volume 5%"
            },

            {
                id: 2,
                name: "Volume Down",
                description: "Lower volume 5%"
            }
        ]
    }

    query(input) {
        const selections = filter_selections(this.default_selections, input.toLowerCase());
        for (const selection of selections) {
            this.send({ "Append": selection})
        }

        this.send("Finished")
    }

    submit(id) {
        let cmd = null

        let sinks = pactl_sinks()

        switch (id) {
            case 0:
                cmd = ["pactl set-sink-mute", "toggle"]
                break
            case 1:
                cmd = ["pactl set-sink-volume", "+5%"]
                break
            case 2:
                cmd = ["pactl set-sink-volume", "-5%"]
        }

        if (cmd) {
            try {
                for (const { id } of sinks) {
                    GLib.spawn_command_line_async(`${cmd[0]} ${id} ${cmd[1]}`)
                }

            } catch (e) {
                log(`session command '${cmd}' failed: ${e}`)
            }
        }
    }

    send(object) {
        STDOUT.write_bytes(new GLib.Bytes(JSON.stringify(object) + "\n"), null)
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
            if ("Exit" === event) {
                break mainloop
            } else if ("Search" in event) {
                app.query(event.Search)
            } else if ("Activate" in event) {
                app.submit(event.Activate)
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

/**
 *
 * @param {Array<string>} argv
 * @returns {null | Process}
 */
function async_process(argv) {
    const { DataInputStream, SubprocessFlags, SubprocessLauncher } = Gio

    try {
        const launcher = new SubprocessLauncher({
            flags: SubprocessFlags.STDIN_PIPE
                | SubprocessFlags.STDOUT_PIPE
        })

        const proc = launcher.spawnv(argv)
        let stdout = new DataInputStream({
            base_stream: proc.get_stdout_pipe(),
            close_base_stream: true
        })

        return { proc, stdout }
    } catch (e) {
        log(`failed to spawn process: ${argv}\n\tCaused by: ${e}`)
        return null
    }
}

function filter_selections(initial, input) {
    if (input.length === 0) return []
    let selections = initial.map(v => ({ ...v }))

    let remove = new Array()
    for (let id = 0; id < selections.length; id += 1) {
        const { name, description } = selections[id]
        if (name.toLowerCase().includes(input) || description.toLowerCase().includes(input)) continue
        remove.push(id)
    }

    for (const id of remove.reverse()) swap_remove(selections, id)

    return selections
}

/**
 *
 * @param {Array<T>} array
 * @param {number} index
 * @returns {T | undefined}
 */
function swap_remove(array, index) {
    array[index] = array[array.length - 1];
    return array.pop();
}

main()