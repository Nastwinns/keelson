# hawplugin (Go)

A thin, stdlib-only Go binding for the [haw plugin contract](../../schemas/).
It parses the `haw.plugin/1` context haw hands you and helps you emit
`haw.plugin.report/1` and `haw.plugin.view/1` documents.

The JSON Schemas in [`schemas/`](../../schemas/) are the source of truth; this
package mirrors them via json struct tags.

## Install

```sh
go get github.com/Nastwinns/hawser/bindings/go/hawplugin
```

## API

- `ReadContext() (Context, error)` — read the context from `HAW_JSON` (falls
  back to stdin). Fail-open: a malformed document returns a schema-only
  `Context` plus the parse error. `Context` exposes `Root`, `Stack`, `Repos`,
  `Phase`, `Intent`, and `IsRender()` (`HAW_RENDER=1` or `Intent=="render"`).
- `NewReport(plugin, ok, summary)` + `(*Report).Emit()` — prints a
  `haw.plugin.report/1` document.
- `Repo`, `Artifact{Path, Kind}`, `Finding{Level, Message}` structs.
- `View(title, lines)` — prints a `haw.plugin.view/1` panel.

## Example

[`example/haw-hello`](example/haw-hello) is a complete plugin using the binding:

```sh
cd bindings/go
go build ./...
go build -o haw-hello ./example/haw-hello
HAW_JSON='{"schema":"haw.plugin/1","repos":[]}' ./haw-hello --format json
```

A minimal plugin:

```go
package main

import "github.com/Nastwinns/hawser/bindings/go/hawplugin"

func main() {
	ctx, _ := hawplugin.ReadContext()
	rep := hawplugin.NewReport("greet", true, "greetings")
	for _, r := range ctx.Repos {
		rep.Findings = append(rep.Findings, hawplugin.Finding{Level: "info", Message: r.Name})
	}
	_ = rep.Emit()
}
```
