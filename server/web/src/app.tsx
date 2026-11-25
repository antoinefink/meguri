// SPDX-License-Identifier: MIT
// sidebar-01 composition adapted from pinned shadcn/ui. See upstream-records.json.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { MouseEvent, ReactNode } from "react";
import { consumeInitialToken, takeFragment } from "./bootstrap";
import { api, asError, errorMessage } from "./api";
import type { Me } from "./api";
import { navigation, protectedPath } from "./policy";
import { CredentialsForm } from "./forms";
import type { FormMode } from "./forms";
import { Accounts } from "./accounts";
import { Collection } from "./collection";
import { Connection } from "./connection";
import { Consent } from "./consent";
import { Button, Card, CardContent, CardDescription, CardHeader, CardTitle, Outcome, Sidebar, SidebarContent, SidebarGroup, SidebarHeader, SidebarInset, SidebarMenu, SidebarMenuButton, SidebarMenuItem } from "./ui";
import type { OutcomeValue } from "./ui";
import pomegranate from "./favicon-64.png";

const accountHomePath = "/collection";

function Brand({ auth = false, onClick }: { auth?: boolean; onClick?: (event: MouseEvent<HTMLAnchorElement>) => void }) {
  const content = <>
    <img src={pomegranate} alt="" width={28} height={28} />
    <span>Meguri</span>
  </>;
  return auth ? <p className="brand auth-brand">{content}</p>
    : <a className="brand" href={accountHomePath} onClick={onClick}>{content}</a>;
}

// Original decorative identity and sign-out drawings; no account menu.
function IdentityIcon() {
  return <svg className="identity-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" aria-hidden="true" focusable="false">
    <circle cx="12" cy="8" r="3.5" /><path d="M5 21v-2a7 7 0 0 1 14 0v2" />
  </svg>;
}
function SignOutIcon() {
  return <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" focusable="false">
    <path d="M9 4H4v16h5m8-12 4 4-4 4M8 12h13" />
  </svg>;
}

// Original, decorative line drawings for the account destinations.
function NavigationIcon({ path }: { path: string }) {
  const drawing = path === "/connection" ? <><path d="M3 8h6m6 8h6M7 4v8m10 0v8M9 8h3v8h3" /></>
    : path === "/collection" ? <><ellipse cx="12" cy="5" rx="8" ry="3" /><path d="M4 5v14c0 4 16 4 16 0V5M4 12c0 4 16 4 16 0" /></>
    : path === "/settings" ? <><circle cx="8" cy="9" r="4" /><path d="m11 12 8 8m-3-3 3-3m-6 0 3-3" /></>
    : <><circle cx="8" cy="7" r="3" /><path d="M2 20v-3a6 6 0 0 1 12 0v3m2-16a3 3 0 0 1 0 6m2 3a5 5 0 0 1 4 5v2" /></>;
  return <svg className="navigation-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" focusable="false">{drawing}</svg>;
}

type PageState = { path: string; token: string; visit: number };
export function App() {
  const [page, setPage] = useState<PageState>(() => {
    const token = consumeInitialToken();
    return { path: location.pathname, token: ["/setup", "/invite", "/reset", "/oauth/consent"].includes(location.pathname) ? token : "", visit: 0 };
  });
  const [me, setMe] = useState<Me | null>(null), [setup, setSetup] = useState(false), [loading, setLoading] = useState(true);
  const [bootstrapError, setBootstrapError] = useState<OutcomeValue | null>(null), [notice, setNotice] = useState<OutcomeValue | null>(null);
  const [signingOut, setSigningOut] = useState(false), logoutBusy = useRef(false);
  const [identityVisit, setIdentityVisit] = useState(0);
  const main = useRef<HTMLElement>(null), startup = useRef(false), alive = useRef(true);
  const go = (path: string, message?: string, replace = false) => {
    if (replace) history.replaceState(null, "", path); else history.pushState(null, "", path);
    setPage(previous => ({ path, token: "", visit: previous.visit + 1 }));
    setNotice(message ? { text: message } : null);
  };
  const clearToken = () => setPage(previous => ({ ...previous, token: "" }));
  const sessionLoss = () => { setMe(null); go("/login", "Your session is no longer available. Sign in again.", true); };
  async function bootstrap() {
    if (startup.current) return;
    startup.current = true; setLoading(true); setBootstrapError(null);
    try {
      const status = await api<{ setup_required: boolean }>("/api/status");
      let identity: Me | null = null;
      try { identity = await api<Me>("/api/me"); }
      catch (caught) { if (asError(caught).status !== 401) throw caught; }
      if (!alive.current) return;
      setSetup(status.setup_required); setMe(identity);
    } catch (caught) {
      if (alive.current) setBootstrapError({ error: true, text: errorMessage(asError(caught)) });
    } finally { startup.current = false; if (alive.current) setLoading(false); }
  }
  useEffect(() => {
    void bootstrap();
    const onHistory = () => {
      const token = takeFragment(location, history);
      setPage(previous => ({ path: location.pathname, token, visit: previous.visit + 1 })); setNotice(null);
    };
    const onHash = () => { if (location.hash) onHistory(); };
    window.addEventListener("popstate", onHistory); window.addEventListener("hashchange", onHash);
    const onPageHide = () => { setPage(previous => ({ ...previous, token: "", visit: previous.visit + 1 })); setMe(null); };
    const onPageShow = (event: PageTransitionEvent) => { if (event.persisted) { takeFragment(location, history); location.reload(); } };
    window.addEventListener("pagehide", onPageHide); window.addEventListener("pageshow", onPageShow);
    return () => {
      alive.current = false;
      window.removeEventListener("popstate", onHistory); window.removeEventListener("hashchange", onHash);
      window.removeEventListener("pagehide", onPageHide); window.removeEventListener("pageshow", onPageShow);
    };
  }, []);
  useEffect(() => {
    if (loading || bootstrapError) return;
    if (page.path === "/setup" && !setup) go("/login", undefined, true);
    else if (page.path === "/") go(setup ? "/setup" : me ? accountHomePath : "/login", undefined, true);
    else if (protectedPath(page.path) && !me) go("/login", "Sign in to open your account.", true);
  }, [loading, bootstrapError, setup, me, page.path]);
  useEffect(() => {
    if (loading || bootstrapError || !me || !protectedPath(page.path)) return;
    const controller = new AbortController();
    void api<Me>("/api/me", undefined, controller.signal).then(identity => {
      if (!controller.signal.aborted) setMe(identity);
    }).catch(caught => {
      if (controller.signal.aborted) return;
      const error = asError(caught);
      if (error.status === 401) sessionLoss();
      else setNotice({ error: true, text: errorMessage(error) });
    });
    return () => controller.abort();
  }, [page.visit, loading, identityVisit]);
  useLayoutEffect(() => {
    if (!loading && document.activeElement?.getAttribute("role") !== "alert") main.current?.focus();
  }, [page.visit, loading]);
  function intercept(event: MouseEvent<HTMLAnchorElement>, path: string) {
    if (event.button || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault(); go(path);
  }
  function link(path: string, label: string): ReactNode { return <a href={path} onClick={event => intercept(event, path)}>{label}</a>; }
  async function logout(keepCapability = false) {
    if (logoutBusy.current) return;
    logoutBusy.current = true; setSigningOut(true); setNotice(null);
    try {
      await api("/api/logout", {});
      if (!alive.current) return;
      setMe(null);
      if (keepCapability) setNotice({ text: "Signed out. You can now use this link." });
      else go("/login", "Signed out.", true);
    } catch (caught) {
      if (!alive.current) return;
      const error = asError(caught);
      if (error.status === 401) {
        setMe(null); if (!keepCapability) go("/login", "Your session has ended.", true);
      } else setNotice({ error: true, text: errorMessage(error) });
    } finally { logoutBusy.current = false; if (alive.current) setSigningOut(false); }
  }
  const capabilityPage = ["/setup", "/invite", "/reset"].includes(page.path);
  const accountShell = me !== null && !capabilityPage && page.path !== "/login" && page.path !== "/oauth/consent";
  let content: ReactNode;
  if (loading) content = <p role="status">Opening Meguri…</p>;
  else if (bootstrapError) content = <Card><CardHeader><CardTitle>Could not open your account</CardTitle></CardHeader><CardContent><Outcome value={bootstrapError} /><Button onClick={() => void bootstrap()}>Try loading again</Button></CardContent></Card>;
  else if (!navigation.some(path => path === page.path)) content = <Card><CardHeader><CardTitle>Page not found</CardTitle><CardDescription>This address does not match a Meguri account page.</CardDescription></CardHeader><CardContent>{link(me ? accountHomePath : "/login", me ? "Go to Collection" : "Go to sign in")}</CardContent></Card>;
  else if (page.path === "/oauth/consent") content = <Consent key={page.visit} request={page.token} clearRequest={clearToken} me={me} onLogin={setMe} onSessionLoss={() => setMe(null)} link={link} />;
  else if (capabilityPage && me) content = <Card className="auth-card"><CardHeader><CardTitle>Check your account</CardTitle><CardDescription>You are signed in as {me.username} ({me.role === "admin" ? "administrator" : "member"}).</CardDescription></CardHeader><CardContent className="stack"><p>This link may be intended for a different account. Sign out before using it.</p><Button aria-disabled={signingOut} onClick={() => void logout(true)}>{signingOut ? "Signing out…" : `Sign out ${me.username}`}</Button>{link(accountHomePath, "Return to your account")}</CardContent></Card>;
  else if (page.path === "/accounts" && me?.role !== "admin") content = <Card><CardHeader><CardTitle>Administrator access required</CardTitle><CardDescription>Your member account does not have permission to manage accounts.</CardDescription></CardHeader><CardContent>{link(accountHomePath, "Return to Collection")}</CardContent></Card>;
  else if (page.path === "/accounts" && me) content = <Accounts me={me} sessionLoss={sessionLoss} />;
  else if (page.path === "/collection" && me) content = <Collection key={`${page.visit}:${me.id}`} sessionLoss={sessionLoss} />;
  else if (page.path === "/connection" && me) content = <Connection key={`${page.visit}:${me.id}`} me={me} sessionLoss={sessionLoss} refreshIdentity={() => setIdentityVisit(value => value + 1)} />;
  else if (page.path === "/login" && me) content = <Card className="auth-card"><CardHeader><CardTitle>Signed in as {me.username}</CardTitle></CardHeader><CardContent className="stack">{link(accountHomePath, "Open Collection")}<Button variant="outline" aria-disabled={signingOut} onClick={() => void logout()}>Sign out</Button></CardContent></Card>;
  else if (["/login", "/setup", "/invite", "/reset", "/settings"].includes(page.path) && (!protectedPath(page.path) || me)) content = <CredentialsForm key={page.visit} mode={page.path.slice(1) as FormMode} token={page.token} clearToken={clearToken} onLogin={identity => { setMe(identity); go(accountHomePath, "Signed in.", true); }} onSuccess={message => { setMe(null); if (page.path === "/setup") setSetup(false); go("/login", message, true); }} onSessionLoss={sessionLoss} link={link} />;
  else content = <p role="status">Opening account page…</p>;
  const links: [string, string][] = [["/collection", "Collection"], ["/connection", "Connection"], ["/settings", "Password"], ...(me?.role === "admin" ? [["/accounts", "Accounts"] as [string, string]] : [])];
  const accountIdentity = me && <>
    <IdentityIcon />
    <div className="footer-identity">
      <p className="identity" title={me.username}>{me.username}</p>
    </div>
    <Button className="sign-out" variant="outline" disabled={signingOut}
      title={signingOut ? "Signing out…" : "Sign out"}
      aria-label={signingOut ? "Signing out…" : "Sign out"} onClick={() => void logout()}>
      <SignOutIcon />
    </Button>
  </>;
  return <div className={accountShell ? "account-layout" : "auth-layout"}>
    <a className="skip-link" href="#main" onClick={event => { event.preventDefault(); main.current?.focus(); }}>Skip to content</a>
    {accountShell && <Sidebar>
      <SidebarHeader><Brand onClick={event => intercept(event, accountHomePath)} />
        <div className="sidebar-footer mobile-account" role="group" aria-label="Signed-in account">{accountIdentity}</div>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup aria-label="Account"><SidebarMenu>
          {links.map(([path, label]) => <SidebarMenuItem key={path}>
            <SidebarMenuButton href={path} aria-current={page.path === path ? "page" : undefined} onClick={event => intercept(event, path)}>
              <NavigationIcon path={path} /><span>{label}</span>
            </SidebarMenuButton>
          </SidebarMenuItem>)}
        </SidebarMenu></SidebarGroup>
      </SidebarContent>
      <div className="sidebar-footer desktop-account" role="group" aria-label="Signed-in account">{accountIdentity}</div>
    </Sidebar>}
    <SidebarInset id="main" ref={main} tabIndex={-1}>
      {!accountShell && <Brand auth />}
      <Outcome value={notice} />{content}
    </SidebarInset>
  </div>;
}
