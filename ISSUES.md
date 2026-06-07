# Open Issues

1. Backspace is currently not supported in the code input field.
2. It should be possible to add new registers using `:add`. We need a dialog like the edit one that can be used to add a new register. On cancel it's dropped, on confirmation it's added.
3. If all predefined values are removed in the EditSelectionDialog, it should switch to the EditInputDialog automatically. Also added a predefined value in the EditInputDialog should instantly switch to EditSelectionDialog.
4. The log table should consist of two columns, timestamp and message. Thus, we have to also add timestamps to all log messages, most-likely update ferrowl-log to support it.
5. In the table it should be possible to go to left edge with '0' and to the right end with '$'.

# Bugs
