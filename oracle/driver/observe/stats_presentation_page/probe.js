// SPDX-License-Identifier: AGPL-3.0-or-later
// The graphs-page probe (README.md): runner.py evaluates `(<this>)(plan)`
// every 50 ms. The first call starts the reading; later calls return null
// until it ends, then a JSON text: the reading, or {"error": <code>}.
//
// plan: {kind: "histogram" | "card_counts", title, history_label, range,
//        page_search}
// Reading: {no_data, bins: [{x0, x1, value, drawn, tooltip, query}],
//           table: [[cell, ...]], sectors?: [value, ...]}
// Text is the page's text with U+2068, U+2069 and U+200E removed; nothing is
// computed here but what the page bound to its elements (d3's __data__).
function (plan) {
  const S = window.__oracleProbe || (window.__oracleProbe = { state: "idle" });
  if (S.state === "done") return S.text;
  if (S.state === "running") return null;
  S.state = "running";

  const MARKS = /[⁨⁩‎]/g;
  const clean = (s) => s.replace(MARKS, "");
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const settle = async () => {
    for (let i = 0; i < 4; i++) await sleep(10);
  };
  class ProbeError extends Error {}
  const fail = (code) => {
    throw new ProbeError(code);
  };

  async function served() {
    const resp = await fetch("/_oracle/state", { cache: "no-store" });
    return (await resp.json()).graphs;
  }

  function section(title) {
    const found = [...document.querySelectorAll(".container")].filter((c) => {
      const h = c.querySelector("h1");
      return h && clean(h.textContent).trim() === title;
    });
    return found.length === 1 ? found[0] : null;
  }

  function idle() {
    const spin = document.querySelector(".range-box .spin");
    return spin && !spin.classList.contains("loading");
  }

  function ready(box) {
    if (!box) return false;
    if (plan.kind === "histogram") {
      const nd = box.querySelector("svg .no-data");
      return !!nd && nd.hasAttribute("pointer-events");
    }
    return box.querySelectorAll(".counts-table tr").length > 0 && box.querySelectorAll("svg .counts path").length > 0;
  }

  async function waitFor(test) {
    // The runner's deadline ends a wait that never succeeds.
    for (;;) {
      if (await test()) return;
      await sleep(25);
    }
  }

  function radio(scope, label) {
    const hits = [...scope.querySelectorAll("label")].filter(
      (l) => l.querySelector("input[type=radio]") && clean(l.textContent).trim() === label,
    );
    if (hits.length !== 1) fail("no_control");
    return hits[0].querySelector("input");
  }

  function lines(html) {
    return html.split(/<br\s*\/?>/i).map((part) => {
      const el = document.createElement("div");
      el.innerHTML = part;
      return clean(el.textContent);
    });
  }

  function cellText(td) {
    return clean(td.textContent).replace(/[■ ]/g, "").trim();
  }

  function tableRows(box) {
    return [...box.querySelectorAll("table tr")].map((tr) => {
      const cells = [...tr.querySelectorAll("td")].map(cellText);
      while (cells.length && cells[cells.length - 1] === "") cells.pop();
      return cells;
    });
  }

  async function readBins(box) {
    const prefix = `browserSearch: ${plan.page_search} `;
    const bins = [];
    for (const rect of box.querySelectorAll("svg g.hover-columns rect")) {
      const bound = rect.__data__;
      if (!Array.isArray(bound)) fail("bin_data");
      const bin = bound[0];
      const x0 = bin.x0;
      const x1 = bin.x1;
      // An entry is one card's value (Intervals, Stability) or a
      // [key, count] pair of a map (the other histograms).
      let value = 0;
      for (const e of bin) {
        if (typeof e === "number") value += 1;
        else if (Array.isArray(e) && e.length === 2 && typeof e[0] === "number" && typeof e[1] === "number") value += e[1];
        else fail("bin_entry");
      }
      const drawn = x1 > x0;
      let tooltip = null;
      let query = null;
      if (drawn) {
        rect.dispatchEvent(new MouseEvent("mousemove", { bubbles: true, clientX: 20, clientY: 20 }));
        await settle();
        const tips = document.querySelectorAll(".tooltip");
        if (tips.length !== 1) fail("tooltip");
        tooltip = lines(tips[0].innerHTML);
        window.__oracleCommands.length = 0;
        rect.dispatchEvent(new MouseEvent("click", { bubbles: true }));
        await settle();
        const cmds = window.__oracleCommands.splice(0);
        if (cmds.length > 1) fail("query_count");
        if (cmds.length === 1) {
          if (!cmds[0].startsWith(prefix)) fail("query_prefix");
          query = cmds[0].slice(prefix.length);
        }
        rect.dispatchEvent(new MouseEvent("mouseout", { bubbles: true }));
        await settle();
      }
      bins.push({ x0, x1, value, drawn, tooltip, query });
    }
    return bins;
  }

  async function read() {
    await waitFor(async () => (await served()) >= 1 && idle() && ready(section(plan.title)));
    if (plan.history_label !== null) {
      const input = radio(document.querySelector(".range-box"), plan.history_label);
      if (!input.checked) {
        input.click();
        await waitFor(async () => (await served()) >= 2 && idle());
      }
    }
    await settle();
    let box = section(plan.title);
    if (!ready(box)) fail("no_section");
    if (plan.range !== null) {
      const input = radio(box, plan.range);
      if (!input.checked) input.click();
      await settle();
      if (!input.checked) fail("control_not_set");
    }
    box = section(plan.title);
    const out = {};
    if (plan.kind === "histogram") {
      out.no_data = box.querySelector("svg .no-data").getAttribute("pointer-events") === "all";
      out.bins = out.no_data ? [] : await readBins(box);
      out.table = tableRows(box).map((row) => {
        if (row.length !== 2 || !row[0].endsWith(":")) fail("table_row");
        return [row[0].slice(0, -1), row[1]];
      });
    } else {
      out.no_data = false;
      out.bins = [];
      out.table = tableRows(box.querySelector(".counts-table"));
      out.sectors = [...box.querySelectorAll("svg .counts path")].map((p) => {
        if (!p.__data__ || typeof p.__data__.value !== "number") fail("sector_value");
        return p.__data__.value;
      });
    }
    return out;
  }

  read().then(
    (out) => {
      S.text = JSON.stringify(out);
      S.state = "done";
    },
    (err) => {
      S.text = JSON.stringify({ error: err instanceof ProbeError ? err.message : "probe_exception" });
      S.state = "done";
    },
  );
  return null;
}
