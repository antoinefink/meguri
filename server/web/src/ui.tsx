// SPDX-License-Identifier: MIT
// Adapted from pinned shadcn/ui new-york-v4; see upstream-records.json and notices.
import { useId, useLayoutEffect, useRef, useState } from "react";
import type { ComponentProps, ReactNode } from "react";

export function Card(props: ComponentProps<"section">) { return <section data-slot="card" {...props} />; }
export function CardHeader(props: ComponentProps<"div">) { return <div data-slot="card-header" {...props} />; }
export function CardTitle({ level = 1, ...props }: ComponentProps<"h1"> & { level?: 1 | 2 }) {
  const Heading = level === 1 ? "h1" : "h2";
  return <Heading data-slot="card-title" {...props} />;
}
export function CardDescription(props: ComponentProps<"p">) { return <p data-slot="card-description" {...props} />; }
export function CardContent(props: ComponentProps<"div">) { return <div data-slot="card-content" {...props} />; }
export function FieldGroup(props: ComponentProps<"div">) { return <div data-slot="field-group" {...props} />; }
export function Field(props: ComponentProps<"div">) { return <div data-slot="field" {...props} />; }
export function FieldLabel(props: ComponentProps<"label">) { return <label data-slot="field-label" {...props} />; }
export function FieldDescription(props: ComponentProps<"p">) { return <p data-slot="field-description" {...props} />; }
export function Input(props: ComponentProps<"input">) { return <input data-slot="input" {...props} />; }
export function Button({ variant = "default", type = "button", ...props }: ComponentProps<"button"> & { variant?: "default" | "outline" | "destructive" }) {
  return <button type={type} data-slot="button" data-variant={variant} {...props} />;
}
export function Sidebar(props: ComponentProps<"aside">) { return <aside data-slot="sidebar" {...props} />; }
export function SidebarHeader(props: ComponentProps<"div">) { return <div data-slot="sidebar-header" {...props} />; }
export function SidebarContent(props: ComponentProps<"div">) { return <div data-slot="sidebar-content" {...props} />; }
export function SidebarGroup(props: ComponentProps<"nav">) { return <nav data-slot="sidebar-group" {...props} />; }
export function SidebarMenu(props: ComponentProps<"ul">) { return <ul data-slot="sidebar-menu" {...props} />; }
export function SidebarMenuItem(props: ComponentProps<"li">) { return <li data-slot="sidebar-menu-item" {...props} />; }
export function SidebarMenuButton(props: ComponentProps<"a">) { return <a data-slot="sidebar-menu-button" {...props} />; }
export function SidebarInset(props: ComponentProps<"main">) { return <main data-slot="sidebar-inset" {...props} />; }

export type OutcomeValue = { text: string; error?: boolean; fields?: Record<string, string> };
export function Outcome({ value, prefix }: { value: OutcomeValue | null; prefix?: string }) {
  const summary = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { if (value?.error) summary.current?.focus(); }, [value]);
  return <div ref={summary} tabIndex={-1} role={value?.error ? "alert" : "status"} className={value?.error ? "outcome error" : "outcome"}>
    {value?.text}
    {value?.fields && <ul>{Object.entries(value.fields).map(([name, text]) => <li key={name}><a href={`#${prefix}-${name}`} onClick={e => {
      e.preventDefault(); document.getElementById(`${prefix}-${name}`)?.focus();
    }}>{text}</a></li>)}</ul>}
  </div>;
}
export function TextField({ label, hint, error, ...props }: ComponentProps<"input"> & { label: string; hint?: string | undefined; error?: string | undefined }) {
  const hintId = `${props.id}-hint`, errorId = `${props.id}-error`;
  return <Field data-invalid={Boolean(error)}>
    <FieldLabel htmlFor={props.id}>{label}</FieldLabel>
    <Input {...props} aria-invalid={Boolean(error)} aria-describedby={[hint ? hintId : "", error ? errorId : ""].filter(Boolean).join(" ") || undefined} />
    {hint && <FieldDescription id={hintId}>{hint}</FieldDescription>}
    {error && <p data-slot="field-error" id={errorId}>{error}</p>}
  </Field>;
}
export function CopyValue({ label, value, children }: { label: string; value: string; children?: ReactNode }) {
  const id = useId(), input = useRef<HTMLInputElement>(null);
  const [outcome, setOutcome] = useState("");
  useLayoutEffect(() => { setOutcome(""); }, [value]);
  return <Field>
    <FieldLabel htmlFor={id}>{label}</FieldLabel>
    <div className="copy-line"><Input id={id} ref={input} readOnly value={value} autoComplete="off" spellCheck={false} onFocus={e => e.currentTarget.select()} />
      <Button variant="outline" onClick={async () => {
        try { await navigator.clipboard.writeText(value); setOutcome("Copied."); }
        catch { input.current?.focus(); input.current?.select(); setOutcome("Copy was unavailable. The text is selected; use your system Copy command."); }
      }}>Copy<span className="sr-only"> {label}</span></Button></div>
    {children}<p role="status" className="muted">{outcome}</p>
  </Field>;
}
