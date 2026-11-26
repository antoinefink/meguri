// SPDX-License-Identifier: MIT
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { FormEvent, KeyboardEvent, ReactNode } from "react";
import { asError, browseApi, errorMessage } from "./api";
import type { BrowseCard, BrowseCatalog, BrowseDetail, BrowseScope, BrowseWrite } from "./api";
import { Button, Field, FieldLabel, Input, Outcome } from "./ui";
import type { OutcomeValue } from "./ui";

type OpenFocus = "field" | "heading" | "keep";
const initialScope: BrowseScope = { query: "", field: null, deck_id: null };
export function Browse({ accountId, sessionLoss, refreshIdentity, guardNavigation }: {
  accountId: string; sessionLoss: () => void; refreshIdentity: () => void;
  guardNavigation: (guard: (() => boolean) | null) => void;
}) {
  const [catalog, setCatalog] = useState<BrowseCatalog | null>(null), [scope, setScope] = useState(initialScope);
  const [query, setQuery] = useState(""), [field, setField] = useState(""), [deck, setDeck] = useState("");
  const [cards, setCards] = useState<BrowseCard[]>([]), [cursor, setCursor] = useState<string | null>(null);
  const [detail, setDetail] = useState<BrowseDetail | null>(null), [adding, setAdding] = useState(false);
  const [fields, setFields] = useState<Record<string, string>>({}), [tags, setTags] = useState("");
  const [newDeck, setNewDeck] = useState(""), [newType, setNewType] = useState("");
  const [loading, setLoading] = useState(false), [opening, setOpening] = useState(false), [busy, setBusy] = useState(false);
  const [outcome, setOutcome] = useState<OutcomeValue | null>(null), [conflict, setConflict] = useState(false), [confirmDelete, setConfirmDelete] = useState(false);
  const [blocked, setBlocked] = useState(false), [deleteError, setDeleteError] = useState<string | null>(null);
  const [detailFresh, setDetailFresh] = useState(true), [pagesChecked, setPagesChecked] = useState(0);
  const committedReplay = useRef<AbortController | null>(null);
  const restoreRowFocus = useRef(false), openFocus = useRef<OpenFocus>("field");
  const deleteDialog = useRef<HTMLDialogElement>(null), deleteTrigger = useRef<HTMLButtonElement>(null);
  const examinedThrough = useRef<string | null>(null);
  const alive = useRef(true), locked = useRef(false), writing = useRef(false);
  const listRequest = useRef<AbortController | null>(null), detailRequest = useRef<AbortController | null>(null);
  const list = useRef<HTMLDivElement>(null), editor = useRef<HTMLDivElement>(null), heading = useRef<HTMLHeadingElement>(null);
  const scroll = useRef(0), pageScroll = useRef(0), listFocus = useRef<string | null>(null), matchField = useRef<string | null>(null);
  const currentScope = useRef(scope); currentScope.current = scope;
  const type = catalog?.note_types.find(nt => nt.note_type_id === newType);
  const names = adding ? type?.field_names ?? [] : detail?.field_names ?? [];
  const dirty = adding ? Object.values(fields).some(Boolean) || !!tags : !!detail && (Object.keys(fields).some(name => fields[name] !== detail.note.fields[name]) || tags !== detail.note.tags.join(" "));
  const dirtyRef = useRef(dirty); dirtyRef.current = dirty;
  const mayLeave = () => !writing.current && (!dirtyRef.current || window.confirm("Discard your unsaved note changes?"));
  useEffect(() => {
    guardNavigation(mayLeave);
    const unload = (event: BeforeUnloadEvent) => { if (dirtyRef.current || writing.current) { event.preventDefault(); event.returnValue = ""; } };
    window.addEventListener("beforeunload", unload);
    return () => { guardNavigation(null); window.removeEventListener("beforeunload", unload); };
  }, []);
  function fail(caught: unknown, report = (text: string) => setOutcome({ error: true, text })) {
    if (!alive.current || locked.current) return;
    const error = asError(caught);
    if (error.status === 401) { locked.current = true; sessionLoss(); return; }
    if (error.code === "stale_authority") {
      locked.current = true; setBlocked(true); listRequest.current?.abort(); detailRequest.current?.abort();
      setCards([]); setDetail(null); setCatalog(null); setFields({}); setTags(""); setAdding(false); setConfirmDelete(false);
      setOutcome({ error: true, text: "The signed-in account changed. Browse has been cleared. Open Browse again after your identity refreshes." });
      refreshIdentity(); return;
    }
    if (error.code === "browse_conflict") { setConflict(true); setConfirmDelete(false); }
    else report(errorMessage(error));
  }
  useEffect(() => {
    const controller = new AbortController();
    void (async () => {
      try {
        let all: BrowseCatalog | null = null, next: string | null = null;
        do {
          const result: BrowseCatalog = await browseApi("catalog", { limit: 50, ...(next ? { cursor: next } : {}) }, accountId, controller.signal);
          if (!alive.current || controller.signal.aborted || locked.current) return;
          if (all && result.generation !== all.generation) throw new Error("Catalog changed");
          all = all ? { ...result, decks: [...all.decks, ...result.decks], note_types: [...all.note_types, ...result.note_types] } : result;
          next = result.next_cursor;
        } while (next);
        setCatalog(all); setNewDeck(all?.decks.find(d => !d.filtered && !d.unavailable)?.deck_id ?? ""); setNewType(all?.note_types.find(nt => !nt.unavailable)?.note_type_id ?? "");
      } catch (caught) { if (!controller.signal.aborted) fail(caught); }
    })();
    return () => { alive.current = false; controller.abort(); listRequest.current?.abort(); detailRequest.current?.abort(); };
  }, []);
  function stopList(navigation = false) {
    // A committed write must reconcile this query even if its editor closes.
    if (navigation && listRequest.current === committedReplay.current) return;
    listRequest.current?.abort(); listRequest.current = null;
    committedReplay.current = null;
    setLoading(false);
  }
  function changeScope(next: BrowseScope) {
    stopList(); restoreRowFocus.current = false;
    setCards([]); setCursor(null); examinedThrough.current = null; scroll.current = 0;
    currentScope.current = next;
    setScope(next);
  }
  async function readPage(target: BrowseScope, after: string | null = null, replace = true, restoreThrough: string | null = null, committedMessage?: string) {
    listRequest.current?.abort();
    const controller = new AbortController();
    listRequest.current = controller;
    committedReplay.current = committedMessage ? controller : null;
    setLoading(true); setPagesChecked(0);
    try {
      // Replay the actual examined key range after a mutation, then continue
      // through empty sparse pages until a visible row or exhaustion. Each
      // request is bounded; new queries always cancel, navigation preserves write replay.
      let next = after, rows: BrowseCard[] = [], pages = 0;
      do {
        const result = await browseApi("page", { ...target, limit: 50, ...(next ? { cursor: next } : {}) }, accountId, controller.signal);
        if (!alive.current || controller.signal.aborted || locked.current || listRequest.current !== controller || currentScope.current !== target) return;
        rows = [...rows, ...result.cards]; next = result.next_cursor;
        setPagesChecked(++pages);
        const replaying = restoreThrough !== null && (result.examined_through === null || BigInt(result.examined_through) < BigInt(restoreThrough));
        // Keep the old list's scroll geometry while replaying its range.
        if (replaying && next) continue;
        examinedThrough.current = result.examined_through;
        const visibleRows = rows;
        setCards(previous => replace ? visibleRows : [...previous, ...result.cards]); setCursor(next);
        if (rows.length > 0) break;
      } while (next);
    } catch (caught) {
      if (!controller.signal.aborted && listRequest.current === controller && alive.current && !locked.current && currentScope.current === target) {
        if (committedMessage) refreshFailed(caught, committedMessage);
        else if (asError(caught).code === "browse_conflict") setOutcome({ error: true, text: "The collection changed while paging. Search again to refresh the results. Your open draft is still available." });
        else fail(caught);
      }
    }
    finally { if (alive.current && !controller.signal.aborted && listRequest.current === controller) setLoading(false); }
  }
  useEffect(() => {
    if (locked.current) return;
    void readPage(scope);
  }, [scope]);
  useLayoutEffect(() => {
    if (!detail && !adding) {
      if (list.current) list.current.scrollTop = scroll.current;
      if (restoreRowFocus.current) focusResult();
    } else if (detail) list.current?.querySelector<HTMLElement>('[aria-current="true"]')?.scrollIntoView({ block: "nearest" });
  }, [cards, detail, adding]);
  function focusResult() {
    const buttons = Array.from(list.current?.querySelectorAll<HTMLButtonElement>("button[data-card]") ?? []);
    const selected = buttons.find(el => el.dataset.card === listFocus.current);
    const nearby = listFocus.current ? buttons.find(el => BigInt(el.dataset.card!) > BigInt(listFocus.current!)) ?? buttons.at(-1) : buttons[0];
    (selected ?? nearby ?? document.getElementById("browse-query"))?.focus({ preventScroll: true });
  }
  useLayoutEffect(() => {
    if (!detail && !adding) return;
    const target = openFocus.current; openFocus.current = "field";
    const active = document.activeElement as HTMLButtonElement | null;
    if (target === "keep" && active && editor.current?.contains(active) && !active.disabled) return;
    const input = target === "field" && matchField.current ? Array.from(editor.current?.querySelectorAll<HTMLTextAreaElement>("textarea") ?? []).find(el => el.dataset.field === matchField.current) : null;
    (input ?? heading.current)?.focus();
  }, [detail?.card.card_id, adding]);
  async function open(card: BrowseCard, focus: OpenFocus = "field") {
    if (locked.current || writing.current || !mayLeave()) return;
    detailRequest.current?.abort(); const controller = new AbortController(); detailRequest.current = controller;
    stopList(true); restoreRowFocus.current = false;
    if (!detail && !adding) {
      if (list.current && list.current.offsetParent !== null) scroll.current = list.current.scrollTop;
      pageScroll.current = window.scrollY;
    }
    listFocus.current = card.card_id; matchField.current = card.field; openFocus.current = focus;
    setOpening(true); setOutcome(null); setConfirmDelete(false);
    try {
      const result = await browseApi("detail", { card_id: card.card_id }, accountId, controller.signal);
      if (!alive.current || controller.signal.aborted || locked.current || detailRequest.current !== controller) return;
      if (!("note" in result)) { setOutcome({ error: true, text: result.card.unavailable }); return; }
      setDetail(result); setAdding(false); setFields({ ...result.note.fields }); setTags(result.note.tags.join(" ")); setConflict(false); setDetailFresh(true);
    } catch (caught) { if (!controller.signal.aborted && detailRequest.current === controller) fail(caught); }
    finally { if (alive.current && !controller.signal.aborted && detailRequest.current === controller) setOpening(false); }
  }
  function back() {
    if (!mayLeave()) return;
    stopList(true); restoreRowFocus.current = true;
    detailRequest.current?.abort(); setOpening(false); setDetail(null); setAdding(false); setConfirmDelete(false); setOutcome(null); setFields({}); setTags("");
    requestAnimationFrame(() => {
      window.scrollTo(0, pageScroll.current);
      if (list.current) list.current.scrollTop = scroll.current;
      focusResult();
    });
  }
  function add() {
    if (locked.current || !catalog || !mayLeave()) return;
    stopList(true); restoreRowFocus.current = false;
    detailRequest.current?.abort(); setOpening(false); setAdding(true); setDetail(null); setFields({}); setTags(""); setConflict(false); setConfirmDelete(false); setOutcome(null);
  }
  async function save(event: FormEvent) {
    event.preventDefault(); if (writing.current || opening || locked.current || (adding ? !catalog : !detail) || conflict) return;
    writing.current = true; setBusy(true); setOutcome(null);
    // A list request begun before this mutation cannot republish old excerpts.
    listRequest.current?.abort(); detailRequest.current?.abort(); setLoading(false); setOpening(false);
    const wasAdding = adding, originalDetail = detail;
    const savedFields = { ...fields };
    const proposedTags = [...new Set(tags.split(/\s+/u).filter(Boolean))];
    const restoreThrough = examinedThrough.current;
    let savedMessage: string | null = null;
    try {
      let committed: BrowseWrite;
      if (wasAdding) {
        committed = await browseApi("create", { generation: catalog!.generation, deck_id: newDeck, note_type_id: newType, fields: savedFields, tags: proposedTags }, accountId);
      } else {
        const note = originalDetail!.note;
        const patch = Object.fromEntries(Object.entries(savedFields).filter(([name, value]) => value !== note.fields[name]));
        const has = (items: string[], name: string) => items.some(item => item.toLocaleLowerCase() === name.toLocaleLowerCase());
        const add_tags = proposedTags.filter(tag => !has(note.tags, tag)), remove_tags = note.tags.filter(tag => !has(proposedTags, tag));
        committed = await browseApi("edit", { note_id: note.note_id, revision: note.revision, fields: patch, add_tags, remove_tags }, accountId);
      }
      if (!alive.current || locked.current) return;
      savedMessage = `Note ${committed.note_id} saved.`;
      // Adopt the commit before any refresh can fail. A later edit must use the
      // new revision; deletion waits for a fresh sibling confirmation.
      if (originalDetail && !wasAdding) {
        setDetail({ ...originalDetail, note: { ...originalDetail.note, fields: savedFields, tags: proposedTags, revision: committed.revision } });
        setFields(savedFields); setTags(proposedTags.join(" ")); setDetailFresh(false);
      } else {
        setAdding(false); setFields({}); setTags(""); listFocus.current = null;
      }
      setOutcome({ text: savedMessage });
      // Replay belongs to this query, beyond the editor navigation/write lease.
      void readPage(currentScope.current, null, true, restoreThrough, savedMessage);
      if (originalDetail && !wasAdding) {
        const saved = await browseApi("detail", { card_id: originalDetail.card.card_id }, accountId);
        if (!alive.current || locked.current) return;
        if (!("note" in saved)) throw new Error(saved.card.unavailable);
        setDetail(saved); setFields({ ...saved.note.fields }); setTags(saved.note.tags.join(" ")); setDetailFresh(true);
      } else focusSearch();
    } catch (caught) {
      if (savedMessage) refreshFailed(caught, savedMessage);
      else fail(caught);
    }
    finally { writing.current = false; if (alive.current) setBusy(false); }
  }
  function focusSearch() {
    requestAnimationFrame(() => {
      if (alive.current && !locked.current) document.getElementById("browse-query")?.focus({ preventScroll: true });
    });
  }
  function refreshFailed(caught: unknown, committedMessage: string) {
    if (!alive.current || locked.current) return;
    const error = asError(caught);
    if (error.status === 401 || error.code === "stale_authority") { fail(caught); return; }
    setOutcome({ error: true, text: `${committedMessage} Could not refresh the server copy. ${errorMessage(error)} Refresh the results or load the server copy to continue.` });
  }
  async function remove() {
    if (!detail || !detailFresh || opening || !confirmDelete || writing.current || locked.current) return;
    writing.current = true; setBusy(true); setOutcome(null); setDeleteError(null); listRequest.current?.abort(); detailRequest.current?.abort(); setLoading(false);
    const restoreThrough = examinedThrough.current;
    try {
      await browseApi("delete", { note_id: detail.note.note_id, revision: detail.note.revision, confirmation: detail.confirmation }, accountId);
      if (!alive.current || locked.current) return;
      setDetail(null); setFields({}); setTags(""); setConfirmDelete(false); setOutcome({ text: "Note and its cards deleted." });
      void readPage(currentScope.current, null, true, restoreThrough, "Note and its cards deleted.");
      focusSearch();
    } catch (caught) { fail(caught, text => deleteDialog.current?.open ? setDeleteError(text) : setOutcome({ error: true, text })); }
    finally { writing.current = false; if (alive.current) setBusy(false); }
  }
  const disabled = busy || blocked;
  const editorDisabled = disabled || opening;
  const cardOpen = !!detail || adding;
  const searching = scope.query !== "";
  const position = detail ? cards.findIndex(card => card.card_id === detail.card.card_id) : -1;
  const neighbour = (step: 1 | -1) => {
    if (position < 0) return null;
    for (let i = position + step; i >= 0 && i < cards.length; i += step) if (!cards[i]!.unavailable) return cards[i]!;
    return null;
  };
  const previous = neighbour(-1), next = neighbour(1);
  const title = adding ? "New note" : detail?.card.title || "Untitled note";
  const siblings = detail?.sibling_ids.length ?? 0;
  useEffect(() => {
    const dialog = deleteDialog.current;
    if (!dialog) return;
    if (confirmDelete && !dialog.open) { setDeleteError(null); dialog.showModal(); dialog.querySelector<HTMLButtonElement>("button")?.focus(); }
    else if (!confirmDelete && dialog.open) dialog.close();
  }, [confirmDelete]);
  const banner = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { if (conflict) banner.current?.focus(); }, [conflict]);
  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    const target = event.target as HTMLElement;
    // The search box clears itself on Escape; selects and the dialog own their keys.
    if (target.closest("dialog, select, #browse-query")) return;
    if (event.key === "Escape" && cardOpen) { event.preventDefault(); back(); return; }
    if (target.closest("input, textarea")) return;
    if ((event.key === "ArrowDown" || event.key === "ArrowUp") && detail && !disabled) {
      const card = event.key === "ArrowDown" ? next : previous;
      if (card) { event.preventDefault(); step(card, "heading"); }
    }
  }
  // Holding a step key must not restart the request for the card already opening.
  function step(card: BrowseCard, focus: OpenFocus) { if (!(opening && listFocus.current === card.card_id)) void open(card, focus); }
  const scopeWith = (change: Partial<BrowseScope>) => ({ query, field: field || null, deck_id: deck || null, ...change });
  const saveButton = <Button type="submit" form="browse-note-form" disabled={editorDisabled || conflict || (adding ? !newDeck || !newType : !dirty)}>{busy ? "Saving…" : adding ? "Create note" : "Save changes"}</Button>;
  const loadServerCopy = detail && <Button variant="outline" disabled={disabled} onClick={() => void open(detail.card)}>Load server copy</Button>;
  return <div className={`browse-page ${cardOpen ? "browse-show-detail" : ""}`} onKeyDown={onKeyDown}>
    <div className="page-heading"><h1>Browse</h1><Button disabled={disabled || !catalog} onClick={add}><Icon><path d="M12 5v14M5 12h14" /></Icon>Add note</Button></div>
    <Outcome value={outcome} />
    {blocked ? <p>Use the account navigation to reopen Browse.</p> : <>
      <form className="browse-toolbar" role="search" onSubmit={event => { event.preventDefault(); if (!busy) { setOutcome(null); changeScope(scopeWith({})); } }}>
        <div className="browse-query">
          <Button type="submit" variant="outline" className="browse-icon-button" aria-label="Search" title="Search" disabled={disabled}><Icon><circle cx="11" cy="11" r="6.5" /><path d="M16 16l4.5 4.5" /></Icon></Button>
          <label htmlFor="browse-query" className="sr-only">Search note fields</label>
          <input id="browse-query" type="search" placeholder="Search note fields" value={query} maxLength={1024} onChange={event => setQuery(event.target.value)} />
          {query && <Button variant="outline" className="browse-icon-button browse-clear" aria-label="Clear search" title="Clear search" disabled={disabled} onClick={() => { setQuery(""); setOutcome(null); changeScope(scopeWith({ query: "" })); document.getElementById("browse-query")?.focus(); }}><Icon><path d="M6 6l12 12M18 6L6 18" /></Icon></Button>}
        </div>
        <div className="browse-filter"><label htmlFor="browse-field">Field</label><select id="browse-field" value={field} onChange={event => { setField(event.target.value); if (!busy) changeScope(scopeWith({ field: event.target.value || null })); }} disabled={disabled || !catalog}>
          <option value="">All fields</option>{[...new Set(catalog?.note_types.flatMap(nt => nt.field_names) ?? [])].map(name => <option key={name}>{name}</option>)}</select></div>
        <div className="browse-filter"><label htmlFor="browse-deck">Deck</label><select id="browse-deck" value={deck} onChange={event => { setDeck(event.target.value); if (!busy) changeScope(scopeWith({ deck_id: event.target.value || null })); }} disabled={disabled || !catalog}>
          <option value="">All decks</option>{catalog?.decks.map(d => <option disabled={!!d.unavailable} value={d.deck_id} key={d.deck_id}>{d.name}</option>)}</select></div>
      </form>
      <p role="status" className="browse-status">{loading ? `Loading cards… (${pagesChecked} pages checked)` : opening ? "Opening card…" : `${cards.length} cards shown${cursor ? ", more available" : ""}.`}</p>
      <div className="browse-split">
        <div className={`browse-list-pane ${searching ? "browse-searching" : ""}`}>
          <div className="browse-columns" aria-hidden="true"><span>Note</span><span>Card</span><span>Deck</span>{searching && <span>Match</span>}</div>
          <div className="browse-results" ref={list} onBlur={event => { if (event.relatedTarget && !event.currentTarget.contains(event.relatedTarget as Node)) restoreRowFocus.current = false; }} onScroll={event => { if (!cardOpen && event.currentTarget.offsetParent !== null) scroll.current = event.currentTarget.scrollTop; }}>
            <ul aria-label="Cards">{cards.map(card => <li key={card.card_id}><button type="button" data-card={card.card_id} className="browse-result" aria-current={detail?.card.card_id === card.card_id ? "true" : undefined} disabled={disabled} onClick={() => { if (card.unavailable) setOutcome({ error: true, text: card.unavailable }); else void open(card); }}>
              <span className="browse-cell-title">{card.title || "Untitled note"}</span>
              <span className="browse-cell-template"><span className="sr-only">, </span>{card.template}</span>
              <span className="browse-cell-deck"><span className="sr-only">, </span>{card.deck}</span>
              {card.unavailable ? <span className="browse-cell-match browse-cell-unavailable"><span className="sr-only">, </span>Card {card.card_id} · Note {card.note_id}. {card.unavailable}</span>
                : searching && card.field && <span className="browse-cell-match"><span className="sr-only">, </span><span className="browse-match-field">{card.field}<span className="sr-only">:</span></span> {card.excerpt.map((part, i) => part.matched ? <mark key={i}>{part.text}</mark> : <span key={i}>{part.text}</span>)}</span>}
            </button></li>)}</ul>
            {!loading && !cards.length && <p className="browse-empty">{cursor ? "No matches in this page. Continue searching." : "No cards match these filters."}</p>}
            {cursor && <div className="browse-more"><Button variant="outline" disabled={loading || disabled} onClick={() => void readPage(scope, cursor, false)}>{searching ? "Search further" : "Load more cards"}</Button></div>}
          </div>
        </div>
        {cardOpen && <section className="browse-editor" ref={editor} aria-labelledby="browse-card-title">
          <div className="browse-card-head">
            <div className="browse-card-nav"><Button variant="outline" disabled={busy} onClick={back}><Icon><path d="M14 7l-5 5 5 5" /></Icon>All cards</Button>
            {detail && <div className="browse-step">
              <Button variant="outline" className="browse-icon-button" aria-label="Previous card" title="Previous card (↑)" disabled={disabled || !previous} onClick={() => previous && step(previous, "keep")}><Icon><path d="M7 14l5-5 5 5" /></Icon></Button>
              <Button variant="outline" className="browse-icon-button" aria-label="Next card" title="Next card (↓)" disabled={disabled || !next} onClick={() => next && step(next, "keep")}><Icon><path d="M7 10l5 5 5-5" /></Icon></Button>
            </div>}</div>
            <div className="browse-card-title">
              <h2 id="browse-card-title" ref={heading} tabIndex={-1}>{title}</h2>
              {detail && <p className="muted">{detail.card.template} card · {detail.card.deck} · <span>{siblings} {siblings === 1 ? "card uses" : "cards share"} this note.</span></p>}
            </div>
            <div className="browse-head-actions">
              {adding && <Button variant="outline" disabled={busy} onClick={back}>Cancel</Button>}
              {detail && <Button ref={deleteTrigger} variant="outline" className="browse-icon-button browse-delete" aria-label="Delete note" title="Delete note" disabled={editorDisabled || conflict || dirty || !detailFresh} onClick={() => setConfirmDelete(true)}><Icon><path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13M10 11v6M14 11v6" /></Icon></Button>}
              <span className="browse-wide-only">{!conflict && loadServerCopy}{saveButton}</span>
            </div>
          </div>
          {conflict && <div ref={banner} tabIndex={-1} role="alert" className="outcome error browse-banner">
            {adding ? <><p>The collection changed. Your draft is still here. Copy any values you need, then reload Browse to get the current collection choices.</p><Button variant="outline" onClick={() => { if (mayLeave()) location.reload(); }}>Reload Browse</Button></>
              : <><p>This note changed after you opened it. Your draft is still here, but it can’t be saved over the newer copy. Copy anything you want to keep, then load the server copy.</p>{loadServerCopy}</>}
          </div>}
          <div className="browse-card-body">
            <form id="browse-note-form" className="stack browse-note-form" onSubmit={event => void save(event)}>
              {adding && <div className="browse-new-choices"><Field><FieldLabel htmlFor="new-deck">Deck</FieldLabel><select id="new-deck" value={newDeck} disabled={editorDisabled} required onChange={event => setNewDeck(event.target.value)}>{catalog?.decks.filter(d => !d.filtered && !d.unavailable).map(d => <option value={d.deck_id} key={d.deck_id}>{d.name}</option>)}</select></Field>
                <Field><FieldLabel htmlFor="new-type">Note type</FieldLabel><select id="new-type" value={newType} disabled={editorDisabled} required onChange={event => { if (!mayLeave()) return; setNewType(event.target.value); setFields({}); setTags(""); }}>{catalog?.note_types.map(nt => <option disabled={!!nt.unavailable} value={nt.note_type_id} key={nt.note_type_id}>{nt.name}</option>)}</select></Field></div>}
              {names.map((name, i) => <Field key={name}><FieldLabel htmlFor={`note-field-${i}`}>{name}</FieldLabel><textarea id={`note-field-${i}`} data-field={name} className={codeLike(detail?.note.fields[name]) ? "browse-code" : undefined} value={fields[name] ?? ""} disabled={editorDisabled} rows={2} onChange={event => { setConfirmDelete(false); setFields(current => ({ ...current, [name]: event.target.value })); }} /></Field>)}
              <Field><FieldLabel htmlFor="note-tags">Tags</FieldLabel><Input id="note-tags" value={tags} disabled={editorDisabled} onChange={event => { setConfirmDelete(false); setTags(event.target.value); }} /><p className="muted">Separate tags with spaces. Field markup is preserved as written.</p></Field>
            </form>
            {detail && <aside className="browse-info-pane" aria-labelledby="browse-info-title"><h3 id="browse-info-title">Card info</h3><dl className="browse-info"><dt>Card ID</dt><dd>{detail.card.card_id}</dd><dt>Note ID</dt><dd>{detail.note.note_id}</dd><dt>Template</dt><dd>{detail.card.template}</dd><dt>Deck</dt><dd>{detail.card.deck}</dd></dl></aside>}
          </div>
          <div className="browse-bottom-bar">{!conflict && loadServerCopy}{saveButton}</div>
          {detail && <dialog ref={deleteDialog} className="browse-dialog" aria-labelledby="browse-delete-title" onCancel={event => { if (busy) event.preventDefault(); }} onClose={() => { setConfirmDelete(false); if (conflict) banner.current?.focus(); else deleteTrigger.current?.focus(); }}>
            <h2 id="browse-delete-title">Delete “{title}”?</h2>
            <p>This removes the note and {siblings === 1 ? "its only card" : `all ${siblings} of its cards`}. Your devices receive the deletion when they sync.</p>
            {deleteError && <p role="alert" className="error">{deleteError}</p>}
            <div className="actions"><Button variant="outline" disabled={busy} onClick={() => setConfirmDelete(false)}>Cancel</Button><Button variant="destructive" disabled={busy || opening || dirty || !detailFresh} onClick={() => void remove()}>Delete note</Button></div>
          </dialog>}
        </section>}
      </div>
    </>}
  </div>;
}
// Markup and media references read better in a code face; judged from the server copy.
function codeLike(value: string | undefined) { return !!value && /<\/?[a-z][^>]*>|\[sound:|&[a-z#0-9]+;/i.test(value); }
function Icon({ children }: { children: ReactNode }) { return <svg aria-hidden="true" viewBox="0 0 24 24" className="browse-icon">{children}</svg>; }
