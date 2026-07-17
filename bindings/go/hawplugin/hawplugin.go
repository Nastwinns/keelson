// Package hawplugin is a thin, stdlib-only Go binding for the haw plugin
// contract.
//
// haw dispatches "haw <name> ..." to a "haw-<name>" executable on PATH, passing
// the workspace context as a haw.plugin/1 JSON document on the HAW_JSON
// environment variable and on stdin. A plugin prints a haw.plugin.report/1
// (for lifecycle phases) or a haw.plugin.view/1 (for TUI render intent)
// document to stdout.
//
// The JSON Schemas in schemas/ are the source of truth for these shapes; this
// package mirrors them via json struct tags.
package hawplugin

import (
	"encoding/json"
	"io"
	"os"
)

// Schema markers for the contract documents.
const (
	// Contract is the context schema haw passes to plugins.
	Contract = "haw.plugin/1"
	// ReportSchema is the report schema plugins emit for lifecycle phases.
	ReportSchema = "haw.plugin.report/1"
	// ViewSchema is the view schema plugins emit under render intent.
	ViewSchema = "haw.plugin.view/1"
)

// Repo is one resolved repo entry in the haw.plugin/1 context.
type Repo struct {
	Name   string   `json:"name"`
	Path   string   `json:"path"`
	Rev    string   `json:"rev"`
	Groups []string `json:"groups"`
}

// Context is the parsed haw.plugin/1 document handed to the plugin.
type Context struct {
	Schema string `json:"schema"`
	Root   string `json:"root,omitempty"`
	Stack  string `json:"stack,omitempty"`
	Repos  []Repo `json:"repos,omitempty"`
	Phase  string `json:"phase,omitempty"`
	Intent string `json:"intent,omitempty"`
}

// IsRender reports whether haw is asking for a human-readable TUI panel,
// signalled by HAW_RENDER=1 in the environment or intent="render" in the
// context document.
func (c Context) IsRender() bool {
	if os.Getenv("HAW_RENDER") == "1" {
		return true
	}
	return c.Intent == "render"
}

// ReadContext reads the haw.plugin/1 context from HAW_JSON, falling back to
// stdin. It is fail-open: a missing or malformed document yields a schema-only
// context with a nil error, mirroring haw's own behaviour outside a workspace.
func ReadContext() (Context, error) {
	ctx := Context{Schema: Contract}

	body := os.Getenv("HAW_JSON")
	if body == "" {
		if b, err := io.ReadAll(os.Stdin); err == nil {
			body = string(b)
		}
	}
	if body == "" {
		return ctx, nil
	}
	if err := json.Unmarshal([]byte(body), &ctx); err != nil {
		// Fail open: keep the schema-only context, report the parse error so
		// callers may log it if they wish.
		return Context{Schema: Contract}, err
	}
	if ctx.Schema == "" {
		ctx.Schema = Contract
	}
	return ctx, nil
}

// Artifact is one artifact a plugin produced (haw.plugin.report/1 artifacts[]).
type Artifact struct {
	Path string `json:"path"`
	// Kind is conventionally one of: sbom, signature, provenance, log, report.
	Kind string `json:"kind"`
}

// Finding is one finding a plugin surfaced (haw.plugin.report/1 findings[]).
type Finding struct {
	// Level is one of: info, warn, error.
	Level   string `json:"level"`
	Message string `json:"message"`
}

// Report is a haw.plugin.report/1 document a plugin emits for a lifecycle phase.
type Report struct {
	Schema    string     `json:"schema"`
	Plugin    string     `json:"plugin"`
	Phase     *string    `json:"phase"`
	OK        bool       `json:"ok"`
	Summary   string     `json:"summary"`
	Artifacts []Artifact `json:"artifacts"`
	Findings  []Finding  `json:"findings"`
}

// NewReport builds a report with the schema marker set and non-nil slices.
func NewReport(plugin string, ok bool, summary string) *Report {
	return &Report{
		Schema:    ReportSchema,
		Plugin:    plugin,
		OK:        ok,
		Summary:   summary,
		Artifacts: []Artifact{},
		Findings:  []Finding{},
	}
}

// Emit prints the report as haw.plugin.report/1 JSON to stdout.
func (r *Report) Emit() error {
	r.Schema = ReportSchema
	if r.Artifacts == nil {
		r.Artifacts = []Artifact{}
	}
	if r.Findings == nil {
		r.Findings = []Finding{}
	}
	return emit(r)
}

// view is the haw.plugin.view/1 panel document.
type viewDoc struct {
	Schema string   `json:"schema"`
	Title  string   `json:"title"`
	Lines  []string `json:"lines"`
}

// View prints a haw.plugin.view/1 panel document to stdout, for use under
// render intent (Context.IsRender()).
func View(title string, lines []string) error {
	if lines == nil {
		lines = []string{}
	}
	return emit(viewDoc{Schema: ViewSchema, Title: title, Lines: lines})
}

func emit(v any) error {
	b, err := json.Marshal(v)
	if err != nil {
		return err
	}
	_, err = os.Stdout.Write(append(b, '\n'))
	return err
}
