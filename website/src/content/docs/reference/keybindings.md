---
title: Keybindings
description: Every shortcut in the sidebar, the worktree spawn modal, and the close-pane modal.
---

## Sidebar

| Key            | Action                                                        |
| -------------- | ------------------------------------------------------------- |
| `prefix + e`   | Toggle sidebar                                                |
| `prefix + E`   | Toggle sidebar in all windows                                 |
| `j` / `Down`   | Move selection down                                           |
| `k` / `Up`     | Move selection up                                             |
| `h` / `Left`   | Previous status filter                                        |
| `l` / `Right`  | Next status filter                                            |
| `r`            | Open repo filter popup                                        |
| `Enter`        | Jump to the selected pane                                     |
| `Tab`          | Cycle status filter                                           |
| `Shift+Tab`    | Switch bottom panel tab (Activity ⇄ Git)                      |
| `Esc`          | Return focus or close the popup                               |

## Optional tmux bindings

The plugin does not reserve next/previous-agent keys by default. Add your own bindings if you want to jump directly between running agents without opening the sidebar:

```tmux
bind-key C-n run-shell '"#{@agent_sidebar_bin}" focus next --scope all'
bind-key C-p run-shell '"#{@agent_sidebar_bin}" focus prev --scope all'
bind-key M-n run-shell '"#{@agent_sidebar_bin}" focus next --scope session'
bind-key M-p run-shell '"#{@agent_sidebar_bin}" focus prev --scope session'
```

`@agent_sidebar_bin` is set by the plugin to the binary it loaded, so these bindings work without the binary being on your `PATH`. tmux expands the format when the key is pressed, and the surrounding double quotes keep paths containing spaces intact. Place the bindings after the plugin is loaded in your `tmux.conf`.

`--scope all` navigates running agents across every tmux session. `--scope session` limits navigation to the session containing the currently active pane. Navigation wraps at the ends of the eligible list.

## Repo filter popup

Opened with `r` or by clicking the repo filter button in the sidebar header.

| Key           | Action                                 |
| ------------- | -------------------------------------- |
| `j` / `Down`  | Move selection down                    |
| `k` / `Up`    | Move selection up                      |
| `Enter`       | Confirm — filter the list to that repo |
| `Esc`         | Cancel                                 |

## Notices popup

Opened by clicking the `ⓘ` badge shown when hooks or plugin setup are missing.

| Key   | Action          |
| ----- | --------------- |
| `Esc` | Close the popup |

## Worktree

| Key | Action                                 |
| --- | -------------------------------------- |
| `n` | Spawn a new worktree + agent           |
| `x` | Remove the selected spawn-created pane |

## Spawn worktree modal

Opened with `n` on a repo.

| Key                                | Action                                                                                           |
| ---------------------------------- | ------------------------------------------------------------------------------------------------ |
| Text keys                          | Type the name (used as the branch slug and tmux window name)                                     |
| `↑` / `↓` / `Tab` / `Shift+Tab`    | Move focus between `NAME` / `AGENT` / `MODE` fields                                              |
| `←` / `→`                          | Cycle the value when the agent or mode field has focus                                           |
| `Enter`                            | Create the worktree + window and launch the agent                                                |
| `Esc`                              | Cancel                                                                                           |

## Close pane modal

Opened with `x` on a spawn-created pane.

| Key             | Action                                                                                                    |
| --------------- | --------------------------------------------------------------------------------------------------------- |
| `y` / `Enter`   | Close the tmux window, remove the git worktree (`--force`), and delete the branch (`git branch -D`)       |
| `c`             | Close the tmux window only, keep the worktree and branch on disk                                          |
| `n` / `Esc`     | Cancel                                                                                                    |
