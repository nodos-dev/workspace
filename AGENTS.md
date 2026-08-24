# AGENTS.md — Nodos Engine & Plugin Development

Workspace for developing Nodos Engine and its plugins/subsystems. `nodos.exe` (built from `Toolchain/nosman`) is the
package manager and build driver.

## Layout
- `Engine/` — Engine sources. May contain different version lines.
- `Module/` — Plugin sources, Contains sub-repos and `Downloaded/` folder for fetched deps.
- `Project*/` — Generated project trees (CMake output + runtime). Disposable; regenerate, don't hand-edit.
- `Toolchain/` — Workspace CMake entrypoint (`Toolchain/CMake`) and nosman.

## Anatomy of a plugin
A plugin is a folder containing:
- `*.nosplugin` (`*.noscfg` or `*.nossys` for Nodos 1.3) — manifest: `info.id.{name,version}`, `dependencies` (name + version), `sdk_version`,
  `schema_version`. Versions are semver; this `version` is what gets published.
- `Source/` — C++ implementation; one `*.cpp` per node plus a registerer.
- `Nodes/*.nosnode`, (`*.nosdef` for Nodos 1.3) — node graph/type definitions.
- `Include/`, `Types/` (`.fbs`) — public headers and FlatBuffers schemas. `flatc` output lands in `Include/<Plugin>`.
- `CMakeLists.txt` — required on 1.3 and earlier; optional on 1.4+ (only for extra sources/build logic).

## Build
```
./nodos dev gen                      # generate project files (scans Module/) to ./Project
./nodos dev build                    # build all for default project (./Project)
./nodos dev build -p Project13 --target <name>   # targeted build in a specific project
```
Out-of-tree plugin: `./nodos dev gen -p <project> --plugin-dirs "<repo>"` then
`./nodos dev build -p <project>`.

## SDK lines
- **1.4+**: `.nosplugin` manifests auto-generate CMake targets; no per-plugin CMake needed. The toolchain
  fetches the declared SDK + package deps. Share deps via `NosPluginCommon.cmake` (`nos_plugin_common`);
  extend a generated target via a sibling `CMakeLists.txt` (toolchain sets `NOS_PLUGIN_TARGET`).
- **1.3 and earlier (legacy)**: per-plugin `CMakeLists.txt` using `nos_find_sdk`, `nos_get_module`,
  `nos_add_plugin`, `nos_generate_flatbuffers`.

## Node implementation
- Bind a node with `NOS_BIND_NODE_CLASS(NOS_NAME("..."), ContextStruct, fn)`. The macro auto-wires the
  context's `ExecuteNode` override and a static `GetFunctions` (no manual hookup). Register each node from the
  plugin's `ExportNodeFunctions` (one `case` per node enum).
- Callable buttons ("functions within a node") = entries in the nosdef `functions[]` array, bound by the static
  `GetFunctions(count, names, fns)` returning `nosName` + `nosPfnNodeFunctionExecute` lambdas (`void* ctx` is the context).
- For Nodos 1.3: The manifest's `node_definitions` array must list every `*.nosdef` explicitly — nosdefs are NOT auto-discovered in Nodos 1.3.
  Adding a node and registering its class isn't enough; without the manifest entry the node never appears.
- `nos_add_plugin` globs `Source/` recursively (`CONFIGURE_DEPENDS`), so a new `.cpp` needs no CMake edit.

## Conventions
- C++ constants: `SCREAMING_SNAKE_CASE` (never `kCamelCase`).
- `always_execute`: leave false (default) if the node does not need executing when no input is dirty, and
  `SetPinValue` on an input pin (e.g. from a function) dirties the node and re-triggers `ExecuteNode`. Set it true
  only when output must refresh without an input change: time-varying sources, per-frame side effects / edge
  detection, threaded sources, or a function that mutates internal state without writing any pin.
- (For Nodos 1.4+) Object lifecycle: use `nos::ObjectRef` / `nos::TypedObjectRef`, not raw `nosObjectId`/C structs.
- Versioning: on an API break, bump major in the affected `*.nosplugin`. Don't touch dependency versions
  unless that's the task.

## Commit messages

Subject: one line, verb first, naming the thing changed or the fault fixed.
`Fix crash when a plugin fails to load`, not `Stop the engine falling over when
a plugin will not load`. Around 50 to 70 characters, sentence case, no trailing
period, no area prefix.

Body: what was wrong, then why the change is what it is. Do not over-explain, use concise and simple language. Do not restate a code comment.

- No `@` anywhere; GitHub reads it as a mention.
- Ports and cherry-picks: prefix the subject with `From <source branch>:` and
  end the body with `(cherry-picked from <sha>)`.
- Only `Toolchain/nosman` changes take a prefix, `nosman:`.
- One commit does one thing.
- The pre-commit hook reformats and re-stages whole files, so staging a single
  hunk does not survive it.
