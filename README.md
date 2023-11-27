# hyprwatch
*hyprwatch* is a CLI abstraction over [Hyprland](https://github.com/hyprwm/hyprland)'s [event socket](https://wiki.hyprland.org/IPC/#tmphyprhissocket2sock) which makes it easier to use and adds some extra data.

## Premise 
This program is very simple. If it is executed, it starts to listen on the event socket for relevant events. Should such an event occur, it loads data from the normal socket (which hyprctl uses), and prints that to the terminal. This input can then be piped into other utilities or used by a custom script. The output of hyprwatch is also enhanced with custom attributes which are not accessible over hyprctl.

The following features are currently implemented:
- Listening to changes in `monitors`, `workspaces` or `clients`.
- More attributes in `workspaces` like whether it is focused, shown on a monitor, or even exists.
- The monitor name as an attribute in `clients`.
- Loading empty workspaces from your Hyprland configuration and including them in `workspaces`.
- Similar command syntax to `hyprctl`.
- Future-proof and efficient usage of the two sockets.

Use-cases include:
- Using it to listen to workspace events with [eww](https://github.com/elkowar/eww).
- Piping the output to a custom script.
- Many more.

## Usage
The syntax of *hyprwatch* is inspired by hyprctl and at its core uses the same subcommands to retrieve data. So use:
- `hyprwatch monitors` to watch for changes on the monitors.
- `hyprwatch workspaces` to watch for changes in the workspaces.
- `hyprwatch clients` to watch for changes in the clients.

The following general options are available to be specified *before* the subcommand:
- `-p / --pretty` - Pretty print the JSON so it spans over multiple lines and is human readable.
- `-o / --once` - Only run once and do not listen for events.

For more information, refer to the help page with `--help`.

### Basic Filtering
Currently, *hyprwatch* supports some basic filtering, based on the monitor, or when applicable, workspace of the retrieved entity. This can be done over subcommand specific options (specified *after* the subcommand). Because they are specific to the subcommand, filtering by monitor can only be done on `workspaces` and `clients`, and filtering by workspace can only be done on `clients`. Filtering by whether it is special can only be done on `workspaces`. The filters work like this:

- `-m / --monitor <MONITOR>` - Only returns entities on the provided monitor. The `MONITOR` is the string identifier (aka name) of the monitor.
- `-w / --workspace <WORKSPACE>` - Only returns entities on the given workspace. The `WORKSPACE` can either be a workspace ID, or `name:` followed by the workspace name.
- `-s / --special <SPECIAL>` - Only returns special entities. The `SPECIAL` is a boolean specifying to get only specials or the opposite.

## Additional Attributes
As mentioned, *hyprwatch* also adds a few new attributes to the entities, which are not included with hyprctl. These are mostly based on other data which is retrieved from socket one.

On `workspaces`, the new attributes include:
- `shown: boolean` - Is the workspace currently shown on its monitor.
- `active: boolean` - Is the workspace not only shown but also focused.

On `clients`, the following attribute was added:
- `monitorName: string` - Name of the monitor the client is on.

## Installation
To install *hyprwatch*, download this repo and build it from source. Make sure to have the rust toolchain properly installed. Run the following:

```shell
git clone https://github.com/VirtCode/hyprwatch
cargo install --path ./hyprwatch
```
## License
*hyprwatch* is licensed under the MIT license. Refer to the `LICENSE.txt` file for more information.


