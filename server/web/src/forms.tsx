// SPDX-License-Identifier: MIT
// login-01's card/header/content/FieldGroup composition, adapted for account APIs.
import { useEffect, useId, useRef, useState } from "react";
import type { FormEvent, ReactNode } from "react";
import { api, asError, errorMessage } from "./api";
import type { ErrorField, Me } from "./api";
import { canonicalUsername, passwordError, usernameError } from "./policy";
import { Button, Card, CardContent, CardDescription, CardHeader, CardTitle, FieldDescription, FieldGroup, Outcome, TextField } from "./ui";
import type { OutcomeValue } from "./ui";
export type FormMode = "login" | "setup" | "invite" | "reset" | "settings";
const titles: Record<FormMode, string> = { login: "Sign in", setup: "Set up Meguri", invite: "Create your account", reset: "Reset your password", settings: "Change password" };
const descriptions: Record<FormMode, string> = {
  login: "Use your Meguri username and password.",
  setup: "Create the first administrator. The one-time setup code stops someone else from claiming your server first.",
  invite: "Choose a username and password to accept your invitation.",
  reset: "Choose a new password for the account associated with this link. This does not enable a disabled account.",
  settings: "Replacing your password signs out every browser and native client using this account."
};
export function CredentialsForm({ mode, token, clearToken, onLogin, onSuccess, onSessionLoss, link }: {
  mode: FormMode; token: string; clearToken: () => void; onLogin: (me: Me) => void;
  onSuccess: (message: string) => void; onSessionLoss: () => void; link: (path: string, label: string) => ReactNode;
}) {
  const prefix = useId();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const [current, setCurrent] = useState("");
  const [manualToken, setManualToken] = useState("");
  const [busy, setBusy] = useState(false), submitting = useRef(false), alive = useRef(true);
  const [outcome, setOutcome] = useState<OutcomeValue | null>(null);
  const claim = mode === "setup" || mode === "invite" || mode === "reset";
  const creation = mode !== "login";
  const hasUsername = mode !== "reset" && mode !== "settings";
  const secret = token || manualToken;
  const errors = outcome?.fields ?? {};
  const passwordField = mode === "reset" || mode === "settings" ? "new_password" : "password";
  useEffect(() => () => { alive.current = false; }, []);
  function clearPasswords() { setPassword(""); setConfirmation(""); setCurrent(""); }
  function fail(fields: Record<string, string>) { setOutcome({ error: true, text: "Please check the form.", fields }); }
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (submitting.current) return;
    const fields: Record<string, string> = {};
    const nameError = hasUsername ? usernameError(username) : undefined;
    if (nameError) fields.username = nameError;
    const pwError = creation ? passwordError(password) : (password ? undefined : "Enter your password.");
    if (pwError) fields[passwordField] = pwError;
    if (creation && password !== confirmation) fields.confirmation = "The passwords must match exactly.";
    if (mode === "settings" && !current) fields.current_password = "Enter your current password.";
    if (claim && !/^[A-Za-z0-9_-]{43}$/.test(secret)) fields.token = mode === "setup" ? "Enter a valid one-time setup code." : "Reopen the original link, or ask your administrator for a new one.";
    if (Object.keys(fields).length) { fail(fields); return; }
    submitting.current = true; setBusy(true); setOutcome(null);
    try {
      switch (mode) {
        case "login": {
          await api("/api/login", { username: canonicalUsername(username), password });
          if (!alive.current) return;
          clearPasswords();
          const identity = await api<Me>("/api/me");
          if (!alive.current) return;
          onLogin(identity);
          break;
        }
        case "settings": await api("/api/password", { current_password: current, new_password: password }); break;
        case "reset": await api("/api/resets/claim", { token: secret, new_password: password }); break;
        case "setup": case "invite":
          await api(mode === "setup" ? "/api/setup" : "/api/invitations/claim", { token: secret, username: canonicalUsername(username), password }); break;
      }
      if (!alive.current) return;
      clearPasswords(); setManualToken(""); clearToken();
      if (mode !== "login") onSuccess(mode === "settings" || mode === "reset" ? "Password replaced. Sign in with your new password. Existing client sign-ins have been revoked." : "Account ready. Sign in with the username and password you chose.");
    } catch (caught) {
      if (!alive.current) return;
      const error = asError(caught);
      if (["authentication_failed", "capability_invalid", "stale_authority"].includes(error.code)) clearPasswords();
      if (error.code === "capability_invalid") { setManualToken(""); clearToken(); }
      if (mode === "settings" && (error.status === 401 || error.code === "stale_authority")) { onSessionLoss(); return; }
      const field: ErrorField | undefined = error.code === "username_unavailable" ? "username" : error.field === "password" ? passwordField : error.field;
      const text = errorMessage(error, claim);
      setOutcome({ error: true, text, ...(field ? { fields: { [field]: field === "new_password" || field === "password" ? (passwordError(password) ?? "Check this password and submit again.") : field === "username" ? (usernameError(username) ?? "Choose another username.") : field === "token" ? (mode === "setup" ? "Check the one-time setup code, or ask the person who set up the server." : "Reopen the original link or ask for a new one.") : "Check this field and submit again." } } : {}) });
    } finally { submitting.current = false; if (alive.current) setBusy(false); }
  }
  const inputProps = (name: string) => ({ id: `${prefix}-${name}`, name, readOnly: busy, ...(errors[name] ? { error: errors[name] } : {}) });
  return <Card className={mode === "settings" ? "settings-card" : "auth-card"}>
    <CardHeader><CardTitle>{titles[mode]}</CardTitle><CardDescription>{descriptions[mode]}</CardDescription></CardHeader>
    <CardContent><form onSubmit={submit} noValidate aria-busy={busy}>
      <FieldGroup>
        <Outcome value={outcome} prefix={prefix} />
        {mode === "setup" && <TextField {...inputProps("token")} label="One-time setup code" type="password" autoComplete="off" spellCheck={false} value={secret} onChange={e => { clearToken(); setManualToken(e.target.value); }} hint="Find the code in the server’s first-start console or Docker logs, or in the protected state/setup-token file." />}
        {claim && mode !== "setup" && <p id={`${prefix}-token`} tabIndex={-1} className={errors.token ? "error" : "muted"}>{errors.token ?? (secret ? "Secure link received. It has been removed from the address bar." : "Open the original invitation or reset link to continue.")}</p>}
        {hasUsername && <TextField {...inputProps("username")} label="Username" autoComplete="username" autoCapitalize="none" spellCheck={false} value={username} onChange={e => setUsername(e.target.value)} hint="3–64 letters, numbers, dots, underscores or hyphens." />}
        {mode === "settings" && <TextField {...inputProps("current_password")} label="Current password" type="password" autoComplete="current-password" value={current} onChange={e => setCurrent(e.target.value)} />}
        <TextField {...inputProps(passwordField)} label={creation ? "New password" : "Password"} type="password" autoComplete={creation ? "new-password" : "current-password"} value={password} onChange={e => setPassword(e.target.value)} {...(creation ? { hint: "Use a long password or passphrase. Spaces and letter case are preserved." } : {})} />
        {creation && <TextField {...inputProps("confirmation")} label="Confirm new password" type="password" autoComplete="new-password" value={confirmation} onChange={e => setConfirmation(e.target.value)} />}
        <Button type="submit" aria-disabled={busy}>{busy ? "Submitting…" : mode === "settings" ? "Replace password and sign out" : titles[mode]}</Button>
        {mode === "login" && <FieldDescription>Need access or a password reset? Ask your server administrator for a link.</FieldDescription>}
        {claim && link("/login", "Go to sign in")}
      </FieldGroup>
    </form></CardContent>
  </Card>;
}
