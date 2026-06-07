# Open Issues

1. Drop access type completely. All input registers are by definition readonly, all holding registers are readwrite, coils are readwrite, discrete inputs are readonly.
3. Make slave id editable in dialogs
4. Currently the Lua code input field uses the `InputField` widget that can only handle single line inputs
    * Create a new `CodeInputField` that works like a `Table` without heading, and shows a cursor on the active line allowing to manipulate the content on the line directly. The table has to only consist of two columns. First is the line number, second is the content. The cursor editing should only work on the conbtent. Pressing '\n' should create a new line in the table under the current active line. Since it works like an input field, cursor navigation has to happen using the arrow keys. Navigating up or down should switch to the upper or lower line (if possible) and the cursor should keep the horizontal offset, except it is limited by the line's length. Thus, working like in any other text editor.

# Bugs

1. Updating the Lua script in the edit dialog isn't passed through to the Lua state, currently only the initial script is executed and updates are only displayed in the register definition.
2. First edit of a coil register fails because of 'not wriable', afterwards it works
