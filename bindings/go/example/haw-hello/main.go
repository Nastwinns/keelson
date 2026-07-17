// Command haw-hello is an example haw plugin written in Go using the hawplugin
// binding.
//
// Build it as "haw-hello" and drop it on PATH; haw dispatches "haw hello" to it.
//
//	go build -o haw-hello ./bindings/go/example/haw-hello
//	PATH="$PWD:$PATH" haw hello
//
//   - default:        prints a human greeting
//   - --help:         prints usage
//   - --format json:  emits a haw.plugin.report/1 document
//   - render intent:  emits a haw.plugin.view/1 panel (HAW_RENDER=1)
package main

import (
	"fmt"
	"os"

	"github.com/Nastwinns/hawser/bindings/go/hawplugin"
)

const help = `haw-hello (go) — example haw subcommand plugin

USAGE:
    haw hello [OPTIONS]

OPTIONS:
    -h, --help       Print this help
        --format json
                     Print a haw.plugin.report/1 JSON document on stdout
`

func main() {
	os.Exit(run(os.Args[1:]))
}

func run(args []string) int {
	for _, a := range args {
		if a == "-h" || a == "--help" {
			fmt.Print(help)
			return 0
		}
	}

	// Fail-open: a parse error still yields a usable schema-only context.
	ctx, _ := hawplugin.ReadContext()

	if ctx.IsRender() {
		lines := make([]string, 0, len(ctx.Repos))
		for _, r := range ctx.Repos {
			lines = append(lines, fmt.Sprintf("%s  %s", r.Name, r.Rev))
		}
		if len(lines) == 0 {
			lines = []string{"(no repos in context)"}
		}
		_ = hawplugin.View("hello — fleet", lines)
		return 0
	}

	if len(args) >= 2 && args[0] == "--format" && args[1] == "json" {
		rep := hawplugin.NewReport("hello", true, "hello from haw-hello (go)")
		for _, r := range ctx.Repos {
			rep.Findings = append(rep.Findings, hawplugin.Finding{
				Level:   "info",
				Message: fmt.Sprintf("saw repo %s", r.Name),
			})
		}
		_ = rep.Emit()
		return 0
	}

	if ctx.Root != "" {
		fmt.Printf("hello from haw-hello (go) — workspace at %s\n", ctx.Root)
	} else {
		fmt.Println("hello from haw-hello (go) — no workspace here")
	}
	return 0
}
