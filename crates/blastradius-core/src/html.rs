//! The self-contained HTML report: one file, no external resource, the JSON
//! contract embedded and drawn as a force-directed graph. Nodes are sized by
//! inbound edges and coloured by the strongest confidence of those edges;
//! infrastructure is drawn hollow. When a change was analysed, changed
//! services are ringed, reached services keep their colour and the rest is
//! dimmed. Clicking a node shows its edges and their evidence. The layout is
//! a small force simulation written for this file; there is no library.
use crate::analyze::Analysis;
use crate::blast;

pub fn render(analysis: &Analysis) -> String {
    let widest = blast::widest(&analysis.graph, blast::DEFAULT_DEPTH)
        .map(|(service, reached)| serde_json::json!({ "service": service, "reached": reached }));
    let payload = serde_json::json!({
        "analysis": analysis.to_json(),
        "widest": widest,
        "notReached": blast::NOT_REACHED,
    });
    let data = escape_json(&serde_json::to_string(&payload).unwrap_or_default());
    TEMPLATE
        .replace("{{TITLE}}", &escape_html(&analysis.repository))
        .replace("{{DATA}}", &data)
}

/// JSON safe inside a `<script>` element: no `<`, `>` or `&` survive, so
/// `</script>` in a service name cannot end the element early.
pub fn escape_json(json: &str) -> String {
    json.replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const TEMPLATE: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Vernier · {{TITLE}}</title>
<style>
:root{--bg:#0d1117;--panel:#141a23;--line:#232b37;--text:#e6e9ef;--muted:#8b93a7;--observed:#3ddc97;--static:#5aa9ff;--inferred:#ffb454;--uncertain:#8b93a7;--changed:#ff5ca8;--font:ui-sans-serif,system-ui,-apple-system,"Segoe UI",Roboto,Helvetica,Arial,sans-serif;--mono:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}
*{box-sizing:border-box}
html,body{margin:0;height:100%;background:var(--bg);color:var(--text);font:14px/1.45 var(--font)}
body{display:flex;flex-direction:column;height:100vh}
header{display:flex;align-items:baseline;gap:18px;padding:14px 20px;border-bottom:1px solid var(--line);flex-wrap:wrap}
header .brand{font-weight:800;letter-spacing:.18em;font-size:13px}
header .repo{font-family:var(--mono);color:var(--muted)}
header .meta{color:var(--muted);font-size:13px}
header .headline{margin-left:auto;font-weight:700;color:var(--changed)}
header .headline .arrow{color:var(--muted);font-weight:400;padding:0 6px}
main{display:flex;flex:1;min-height:0}
#graph{flex:1;min-width:0;display:block;background:radial-gradient(ellipse at center,#121926 0%,var(--bg) 70%);cursor:grab;touch-action:none}
#graph.dragging{cursor:grabbing}
aside{width:380px;border-left:1px solid var(--line);background:var(--panel);overflow:auto;padding:16px 18px}
aside h2{font-size:11px;letter-spacing:.14em;text-transform:uppercase;color:var(--muted);margin:20px 0 8px}
aside h2:first-child{margin-top:0}
aside .name{font-size:20px;font-weight:700;font-family:var(--mono);margin:0 0 6px;word-break:break-word}
aside .kv{display:grid;grid-template-columns:92px 1fr;gap:3px 10px;font-size:13px;margin:0}
aside .kv dt{color:var(--muted)}aside .kv dd{margin:0;font-family:var(--mono);word-break:break-word}
.badge{display:inline-block;padding:1px 7px;border-radius:9px;font-size:11px;font-weight:600;font-family:var(--mono);color:#0d1117;vertical-align:middle;white-space:nowrap}
.badge.observed{background:var(--observed)}.badge.static{background:var(--static)}.badge.inferred{background:var(--inferred)}.badge.uncertain{background:var(--uncertain)}.badge.changed{background:var(--changed)}.badge.none{background:#2a3242;color:var(--muted)}
.edge-row{padding:8px 0;border-top:1px solid var(--line);font-size:13px}
.edge-row .head{display:flex;gap:8px;align-items:center;font-family:var(--mono);flex-wrap:wrap}
.edge-row .head .type{color:var(--muted)}
.edge-row .ev{color:var(--muted);font-family:var(--mono);font-size:12px;margin:4px 0 0;padding:0;list-style:none}
.edge-row .ev li{white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.hint{color:var(--muted);font-size:13px}
.list{font-family:var(--mono);font-size:12px;color:var(--muted);line-height:1.7;word-break:break-word}
.finding{display:flex;justify-content:space-between;gap:12px;padding:6px 0;border-top:1px solid var(--line);font-size:13px}
.finding .v{font-family:var(--mono);text-align:right}
.finding .v small{display:block;color:var(--muted);font-family:var(--font)}
table.reached{width:100%;border-collapse:collapse;font-size:12px;font-family:var(--mono);table-layout:fixed}
table.reached td{padding:6px 6px 6px 0;vertical-align:top;border-top:1px solid var(--line);overflow-wrap:anywhere}
table.reached td.svc{width:36%}table.reached td.svc small{display:block;color:var(--muted)}
table.reached td.conf{width:27%}
table.reached td.path{color:var(--muted);font-family:var(--font)}
.fixed{font-size:13px;margin:6px 0}
footer{display:flex;gap:18px;padding:8px 20px;border-top:1px solid var(--line);color:var(--muted);font-size:12px;flex-wrap:wrap}
footer .chip{display:inline-flex;align-items:center;gap:6px}
footer .dot{width:10px;height:10px;border-radius:50%;display:inline-block}
footer .ring{width:10px;height:10px;border-radius:50%;display:inline-block;border:2px solid var(--changed)}
footer .hollow{width:10px;height:10px;border-radius:2px;display:inline-block;border:1.5px solid var(--muted)}
footer .dash{width:16px;border-top:2px dashed var(--uncertain);display:inline-block}
footer .note{margin-left:auto}
.node{cursor:pointer}
.node text{font-size:11px;fill:#cfd3dc;pointer-events:none;font-family:var(--mono)}
svg.busy .node text{font-size:10px}
.node.dim{opacity:.2}.edge.dim{opacity:.07}
.node.faded{opacity:.25}.edge.faded{opacity:.06}
.edge{fill:none}
.edge.uncertain{stroke-dasharray:4 4}
.node.hl circle,.node.hl rect{stroke:#fff;stroke-width:2.5}
.node.quiet text{opacity:0}
.node.quiet.near text{opacity:1}
.node.faded.near{opacity:1}
</style>
</head>
<body>
<header>
  <div class="brand">VERNIER</div>
  <div class="repo" id="repo"></div>
  <div class="meta" id="meta"></div>
  <div class="headline" id="headline"></div>
</header>
<main>
  <svg id="graph" role="img" aria-label="Service dependency graph"></svg>
  <aside id="panel"></aside>
</main>
<footer>
  <span class="chip"><span class="dot" style="background:var(--observed)"></span>observed in production</span>
  <span class="chip"><span class="dot" style="background:var(--static)"></span>static</span>
  <span class="chip"><span class="dot" style="background:var(--inferred)"></span>inferred</span>
  <span class="chip"><span class="dash"></span>uncertain</span>
  <span class="chip"><span class="ring"></span>changed</span>
  <span class="chip"><span class="hollow"></span>infrastructure</span>
  <span class="note">node size: inbound edges · click a node for its evidence · drag to move · wheel to zoom</span>
</footer>
<script id="vernier-data" type="application/json">{{DATA}}</script>
<script>
(function () {
  var payload = JSON.parse(document.getElementById('vernier-data').textContent);
  var A = payload.analysis;
  var B = A.blast || null;
  var COLORS = { observed: '#3ddc97', 'static': '#5aa9ff', inferred: '#ffb454', uncertain: '#8b93a7' };
  var RANK = { observed: 3, 'static': 2, inferred: 1, uncertain: 0 };
  var NS = 'http://www.w3.org/2000/svg';
  function esc(s) { return String(s == null ? '' : s).replace(/[&<>"]/g, function (ch) { return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[ch]; }); }
  function plural(n, one, many) { return n + ' ' + (n === 1 ? one : many); }

  // ---- model
  var nodes = A.services.map(function (s) { return { id: s.name, s: s, inbound: [], outbound: [], conf: null, x: 0, y: 0, dx: 0, dy: 0 }; });
  var byId = {};
  nodes.forEach(function (n) { byId[n.id] = n; });
  A.edges.forEach(function (e) {
    e.key = e.source + '|' + e.target + '|' + e.type;
    var t = byId[e.target], s = byId[e.source];
    if (t) { t.inbound.push(e); if (!t.conf || RANK[e.confidence] > RANK[t.conf]) t.conf = e.confidence; }
    if (s) s.outbound.push(e);
  });
  nodes.forEach(function (n) { n.r = 7 + 3 * Math.sqrt(n.inbound.length); });
  var changed = {}, reached = {}, pathKeys = {};
  if (B) {
    B.changed.forEach(function (c) { changed[c.service] = c; });
    B.reached.forEach(function (r) {
      reached[r.service] = r;
      r.path.forEach(function (h) {
        if (h.relation === 'consumes') pathKeys[h.from + '|' + h.to + '|' + h.type] = true;
        else if (h.relation === 'shares-broker') {
          A.edges.forEach(function (e) { if (e.target === h.via && (e.source === h.from || e.source === h.to)) pathKeys[e.key] = true; });
        } else pathKeys[h.to + '|' + h.from + '|' + h.type] = true;
      });
    });
  }

  // ---- header
  var code = A.services.filter(function (s) { return s.role === 'code'; }).length;
  document.getElementById('repo').textContent = A.repository;
  var rt = A.runtime;
  var runtimeText = rt.connected
    ? 'runtime connected (' + (rt.source === 'otel' ? 'OTel' : 'Datadog') + ', ' + rt.services.matched + ' of ' + rt.services.runtime + ' runtime services matched)'
    : 'runtime not connected, static only';
  document.getElementById('meta').textContent = plural(code, 'service', 'services') + ' · ' + plural(A.edges.length, 'edge', 'edges') + ' · ' + runtimeText;
  if (B) {
    document.getElementById('headline').innerHTML = plural(B.summary.changed, 'service', 'services') + ' changed <span class="arrow">→</span> ' + plural(B.summary.reached, 'service', 'services') + ' in the blast radius';
  }

  // ---- layout: a force simulation with a seeded generator, so the same
  // graph draws the same picture every time
  var svg = document.getElementById('graph');
  var W = Math.max(600, svg.clientWidth || 1000), H = Math.max(400, svg.clientHeight || 700);
  svg.setAttribute('viewBox', '0 0 ' + W + ' ' + H);
  var seed = 1234567;
  function rand() { seed |= 0; seed = seed + 0x6D2B79F5 | 0; var t = Math.imul(seed ^ seed >>> 15, 1 | seed); t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t; return ((t ^ t >>> 14) >>> 0) / 4294967296; }
  function layout() {
    var n = nodes.length; if (!n) return;
    var connected = nodes.filter(function (d) { return d.inbound.length || d.outbound.length; });
    var isolated = nodes.filter(function (d) { return !d.inbound.length && !d.outbound.length; });
    var pad = 70;
    // nodes with no edge at all sit in rows along the bottom, out of the way
    var perRow = Math.max(1, Math.floor((W - 2 * pad) / 56) + 1);
    var rows = Math.ceil(isolated.length / perRow);
    var bottom = rows ? rows * 40 + 10 : 0;
    isolated.forEach(function (d, i) {
      var row = Math.floor(i / perRow), col = i % perRow, inRow = Math.min(perRow, isolated.length - row * perRow);
      d.x = inRow === 1 ? W / 2 : pad + col * (W - 2 * pad) / (inRow - 1);
      d.y = H - pad / 2 - bottom + 24 + row * 40;
    });
    var m = connected.length; if (!m) return;
    var boxW = W - 2 * pad, boxH = H - 2 * pad - bottom;
    // Fruchterman-Reingold without walls: repulsion k^2/d, attraction d^2/k
    // along edges, a gravity that holds separate components together; the
    // result is then fitted into the box, so nothing sticks to a border
    var k = Math.sqrt(boxW * boxH / m);
    connected.forEach(function (d, i) {
      var a = i / m * 2 * Math.PI, r = k * Math.sqrt(m) / 2;
      d.x = r * Math.cos(a) + (rand() - 0.5) * k; d.y = r * Math.sin(a) + (rand() - 0.5) * k;
    });
    var t = k * 2;
    for (var iter = 0; iter < 600; iter++) {
      connected.forEach(function (d) { d.dx = 0; d.dy = 0; });
      for (var i = 0; i < m; i++) for (var j = i + 1; j < m; j++) {
        var a = connected[i], b = connected[j];
        var dx = a.x - b.x, dy = a.y - b.y, dist = Math.max(1, Math.sqrt(dx * dx + dy * dy));
        var f = k * k / dist / dist;
        a.dx += dx * f; a.dy += dy * f; b.dx -= dx * f; b.dy -= dy * f;
      }
      A.edges.forEach(function (e) {
        var a = byId[e.source], b = byId[e.target]; if (!a || !b || a === b) return;
        var dx = a.x - b.x, dy = a.y - b.y, dist = Math.max(1, Math.sqrt(dx * dx + dy * dy));
        var f = dist / k;
        a.dx -= dx * f; a.dy -= dy * f; b.dx += dx * f; b.dy += dy * f;
      });
      // gravity holds separate components together; weaker sideways, so
      // the cluster spreads into the wide box rather than a disc
      connected.forEach(function (d) {
        var aspect = Math.min(1, boxH / boxW);
        d.dx -= d.x * 0.4 * aspect * aspect; d.dy -= d.y * 0.4;
        var len = Math.sqrt(d.dx * d.dx + d.dy * d.dy) || 1, step = Math.min(len, t);
        d.x += d.dx / len * step; d.y += d.dy / len * step;
      });
      t = Math.max(0.2, t * 0.985);
    }
    var minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    connected.forEach(function (d) { minX = Math.min(minX, d.x); maxX = Math.max(maxX, d.x); minY = Math.min(minY, d.y); maxY = Math.max(maxY, d.y); });
    var spanX = Math.max(1, maxX - minX), spanY = Math.max(1, maxY - minY);
    var scale = Math.min(boxW / spanX, boxH / spanY, 3.5);
    var offX = pad + (boxW - spanX * scale) / 2, offY = pad + (boxH - spanY * scale) / 2;
    connected.forEach(function (d) { d.x = offX + (d.x - minX) * scale; d.y = offY + (d.y - minY) * scale; });
  }
  layout();

  // ---- drawing
  function el(name, attrs, parent) { var e = document.createElementNS(NS, name); for (var k in attrs) e.setAttribute(k, attrs[k]); if (parent) parent.appendChild(e); return e; }
  var defs = el('defs', {}, svg);
  Object.keys(COLORS).forEach(function (c) {
    var m = el('marker', { id: 'arrow-' + c, viewBox: '0 0 10 10', refX: '9', refY: '5', markerWidth: '6', markerHeight: '6', orient: 'auto' }, defs);
    el('path', { d: 'M0,0 L10,5 L0,10 z', fill: COLORS[c] }, m);
  });
  var view = el('g', { id: 'view' }, svg);
  var gEdges = el('g', {}, view), gNodes = el('g', {}, view);
  var edgeEls = [], nodeEls = {};
  // parallel edges between one pair are offset so both stay visible
  var pairIndex = {};
  A.edges.forEach(function (e) {
    var pair = [e.source, e.target].sort().join('|');
    pairIndex[pair] = pairIndex[pair] || [];
    e.slot = pairIndex[pair].length; pairIndex[pair].push(e);
  });
  A.edges.forEach(function (e) {
    var a = byId[e.source], b = byId[e.target]; if (!a || !b) return;
    var cls = 'edge ' + e.confidence + (B && !pathKeys[e.key] ? ' dim' : '');
    var width = e.observed && e.observed.calls ? Math.min(4, 1.4 + Math.log10(e.observed.calls + 1) * 0.6) : 1.2;
    var line = el('path', { 'class': cls, stroke: COLORS[e.confidence], 'stroke-width': width, 'marker-end': 'url(#arrow-' + e.confidence + ')' }, gEdges);
    var title = el('title', {}, line);
    title.textContent = e.source + ' → ' + e.target + ' (' + e.type + ', ' + e.confidence + (e.observed && e.observed.calls ? ', ' + e.observed.calls + ' calls' : '') + ')';
    edgeEls.push({ e: e, a: a, b: b, el: line, n: pairIndex[[e.source, e.target].sort().join('|')].length });
  });
  // on a big graph only the labels that carry information stay on; the rest
  // appear when their node or a neighbour is hovered
  var busy = nodes.length > 40;
  if (busy) svg.classList.add('busy');
  nodes.forEach(function (n) {
    var infra = n.s.role !== 'code';
    var loud = !busy || (B ? !!(changed[n.id] || reached[n.id]) : (!infra && n.inbound.length >= 2));
    var cls = 'node' + (B && !changed[n.id] && !reached[n.id] ? ' dim' : '') + (loud ? '' : ' quiet');
    var g = el('g', { 'class': cls, 'data-id': n.id }, gNodes);
    var fill = n.conf ? COLORS[n.conf] : '#3a4356';
    var stroke = '#0d1117', sw = 1.5;
    if (B && changed[n.id]) { stroke = '#ff5ca8'; sw = 3.5; fill = '#1b2230'; }
    else if (B && reached[n.id]) { fill = COLORS[reached[n.id].confidence]; }
    if (infra) el('rect', { x: -n.r, y: -n.r, width: 2 * n.r, height: 2 * n.r, rx: 4, fill: B && changed[n.id] ? fill : '#0d1117', stroke: B && changed[n.id] ? stroke : '#8b93a7', 'stroke-width': sw, 'stroke-dasharray': '3 2' }, g);
    else el('circle', { r: n.r, fill: fill, stroke: stroke, 'stroke-width': sw }, g);
    var label = el('text', { y: n.r + 13, 'text-anchor': 'middle' }, g);
    label.textContent = n.id;
    var title = el('title', {}, g);
    title.textContent = n.id + (infra ? ' (infrastructure)' : '') + ' · ' + plural(n.inbound.length, 'inbound edge', 'inbound edges');
    nodeEls[n.id] = g;
  });
  function position() {
    edgeEls.forEach(function (x) {
      var dx = x.b.x - x.a.x, dy = x.b.y - x.a.y, dist = Math.sqrt(dx * dx + dy * dy) || 1;
      var ux = dx / dist, uy = dy / dist;
      var off = (x.e.slot - (x.n - 1) / 2) * 7;
      var px = -uy * off, py = ux * off;
      var x1 = x.a.x + ux * x.a.r + px, y1 = x.a.y + uy * x.a.r + py;
      var x2 = x.b.x - ux * (x.b.r + 5) + px, y2 = x.b.y - uy * (x.b.r + 5) + py;
      x.el.setAttribute('d', 'M' + x1 + ',' + y1 + ' L' + x2 + ',' + y2);
    });
    nodes.forEach(function (n) { nodeEls[n.id].setAttribute('transform', 'translate(' + n.x + ',' + n.y + ')'); });
  }
  position();

  // ---- interaction: hover, click, drag, pan, zoom
  var scale = 1, tx = 0, ty = 0;
  function applyView() { view.setAttribute('transform', 'translate(' + tx + ',' + ty + ') scale(' + scale + ')'); }
  function focus(id) {
    nodes.forEach(function (n) {
      var related = !id || n.id === id || n.inbound.some(function (e) { return e.source === id; }) || n.outbound.some(function (e) { return e.target === id; });
      nodeEls[n.id].classList.toggle('faded', !!id && !related);
      nodeEls[n.id].classList.toggle('hl', !!id && n.id === id);
      nodeEls[n.id].classList.toggle('near', !!id && related);
    });
    edgeEls.forEach(function (x) { x.el.classList.toggle('faded', !!id && x.e.source !== id && x.e.target !== id); });
  }
  var drag = null, pan = null, moved = false;
  function toSvg(evt) { var r = svg.getBoundingClientRect(); return { x: (evt.clientX - r.left) * (W / r.width), y: (evt.clientY - r.top) * (H / r.height) }; }
  svg.addEventListener('pointerdown', function (evt) {
    var g = evt.target.closest ? evt.target.closest('.node') : null;
    var p = toSvg(evt); moved = false;
    if (g) { drag = { n: byId[g.getAttribute('data-id')], ox: p.x, oy: p.y }; }
    else { pan = { x: p.x, y: p.y, tx: tx, ty: ty }; svg.classList.add('dragging'); }
    svg.setPointerCapture(evt.pointerId);
  });
  svg.addEventListener('pointermove', function (evt) {
    var p = toSvg(evt);
    if (drag) { drag.n.x += (p.x - drag.ox) / scale; drag.n.y += (p.y - drag.oy) / scale; drag.ox = p.x; drag.oy = p.y; moved = true; position(); }
    else if (pan) { tx = pan.tx + (p.x - pan.x); ty = pan.ty + (p.y - pan.y); if (Math.abs(p.x - pan.x) + Math.abs(p.y - pan.y) > 3) moved = true; applyView(); }
    else { var g = evt.target.closest ? evt.target.closest('.node') : null; focus(g ? g.getAttribute('data-id') : null); }
  });
  svg.addEventListener('pointerup', function (evt) {
    if (drag && !moved) showNode(drag.n);
    if (pan && !moved) showDefault();
    drag = null; pan = null; svg.classList.remove('dragging');
  });
  svg.addEventListener('pointerleave', function () { focus(null); });
  svg.addEventListener('wheel', function (evt) {
    evt.preventDefault();
    var p = toSvg(evt), factor = evt.deltaY < 0 ? 1.1 : 1 / 1.1, next = Math.max(0.3, Math.min(4, scale * factor));
    tx = p.x - (p.x - tx) * (next / scale); ty = p.y - (p.y - ty) * (next / scale); scale = next; applyView();
  }, { passive: false });

  // ---- panel
  var panel = document.getElementById('panel');
  function badge(conf) { return '<span class="badge ' + esc(conf) + '">' + esc(conf) + '</span>'; }
  function hopWords(h) {
    var calls = h.calls ? ', ' + h.calls + ' calls' : '';
    switch (h.relation) {
      case 'calls': return esc(h.to) + ' calls ' + esc(h.from) + ' (' + esc(h.type) + calls + ')';
      case 'imports': return esc(h.to) + ' imports ' + esc(h.from);
      case 'shares-database': return esc(h.to) + ' shares a database with ' + esc(h.from);
      case 'consumes': return esc(h.to) + ' consumes events from ' + esc(h.from) + (h.calls ? ' (' + h.calls + ' calls)' : '');
      case 'shares-broker': return esc(h.to) + ' shares broker ' + esc(h.via) + ' with ' + esc(h.from);
    }
    return esc(h.relation);
  }
  function pathWords(path) { return path.slice().reverse().map(hopWords).join('; '); }
  function evidence(e) {
    return '<ul class="ev">' + e.evidence.slice(0, 4).map(function (v) {
      return '<li title="' + esc(v.file + (v.line ? ':' + v.line : '') + (v.detail ? '  ' + v.detail : '')) + '">' + esc(v.file) + (v.line ? ':' + v.line : '') + (v.detail ? '  <span>' + esc(v.detail) + '</span>' : '') + '</li>';
    }).join('') + (e.evidence.length > 4 ? '<li>+' + (e.evidence.length - 4) + ' more</li>' : '') + '</ul>';
  }
  function edgeRows(edges, other) {
    if (!edges.length) return '<p class="hint">none</p>';
    return edges.map(function (e) {
      var calls = e.observed && e.observed.calls ? '<span class="type">' + e.observed.calls + ' calls</span>' : '';
      return '<div class="edge-row"><div class="head"><strong>' + esc(e[other]) + '</strong><span class="type">' + esc(e.type) + '</span>' + badge(e.confidence) + calls + '</div>' + evidence(e) + '</div>';
    }).join('');
  }
  function showNode(n) {
    var s = n.s, h = '';
    h += '<p class="name">' + esc(n.id) + '</p>';
    if (B) {
      if (changed[n.id]) h += '<p>' + badge('changed') + ' <span class="hint">' + plural(changed[n.id].files.length, 'file', 'files') + ' in this change</span></p>';
      else if (reached[n.id]) { var r = reached[n.id]; h += '<p>' + badge(r.confidence) + ' <span class="hint">reached at depth ' + r.depth + '</span></p><p class="hint">' + pathWords(r.path) + '</p>'; }
      else if (s.role === 'code') h += '<p class="fixed hint">' + esc(payload.notReached) + '</p>';
    }
    h += '<dl class="kv">';
    h += '<dt>role</dt><dd>' + esc(s.role) + '</dd>';
    if (s.language) h += '<dt>language</dt><dd>' + esc(s.language) + '</dd>';
    if (s.root) h += '<dt>root</dt><dd>' + esc(s.root) + '</dd>';
    if (s.image) h += '<dt>image</dt><dd>' + esc(s.image) + '</dd>';
    if (s.packageName) h += '<dt>package</dt><dd>' + esc(s.packageName) + '</dd>';
    h += '<dt>declared</dt><dd>' + esc(s.evidence.file + (s.evidence.line ? ':' + s.evidence.line : '')) + '</dd>';
    h += '<dt>found by</dt><dd>' + esc(s.discoveredBy) + '</dd>';
    h += '</dl>';
    h += '<h2>Depended on by · ' + n.inbound.length + '</h2>' + edgeRows(n.inbound, 'source');
    h += '<h2>Depends on · ' + n.outbound.length + '</h2>' + edgeRows(n.outbound, 'target');
    panel.innerHTML = h;
  }
  function finding(label, value, sub) { return '<div class="finding"><span>' + label + '</span><span class="v">' + value + (sub ? '<small>' + sub + '</small>' : '') + '</span></div>'; }
  function showFindings() {
    var h = '<h2>Findings</h2>';
    var never = nodes.filter(function (n) { return n.s.role === 'code' && !n.inbound.length; }).map(function (n) { return n.id; });
    h += finding('Never called by another service', plural(never.length, 'service', 'services'), esc(never.slice(0, 8).join(', ')) + (never.length > 8 ? ', …' : ''));
    var most = null;
    nodes.forEach(function (n) { var srcs = {}; n.inbound.forEach(function (e) { srcs[e.source] = 1; }); var c = Object.keys(srcs).length; if (c && (!most || c > most.c)) most = { id: n.id, c: c }; });
    if (most) h += finding('Most connected', esc(most.id), 'touched by ' + plural(most.c, 'service', 'services'));
    if (payload.widest && payload.widest.reached) h += finding('Widest change surface', esc(payload.widest.service), 'a change here reaches ' + plural(payload.widest.reached, 'service', 'services'));
    // the same rule as the terminal report: a database edge between two code
    // services whose evidence names a shared database
    var shared = {};
    A.edges.forEach(function (e) {
      var a = byId[e.source], b = byId[e.target];
      if (e.type !== 'database' || !a || !b || a.s.role !== 'code' || b.s.role !== 'code') return;
      var key = null;
      e.evidence.forEach(function (v) { if (!key && v.detail && v.detail.indexOf('shared database ') === 0) key = v.detail.slice(16).split(' with ')[0]; });
      if (key) { shared[key] = shared[key] || {}; shared[key][e.source] = 1; shared[key][e.target] = 1; }
    });
    var sharedKeys = Object.keys(shared).sort();
    h += finding('Shared databases', sharedKeys.length, esc(sharedKeys.slice(0, 4).map(function (k) { return k + ': ' + Object.keys(shared[k]).sort().join(', '); }).join(' · ')));
    if (rt.connected) {
      var neverObserved = A.edges.filter(function (e) { return !e.observed && e.type !== 'import' && !(e.type === 'database' && byId[e.source] && byId[e.target] && byId[e.source].s.role === 'code' && byId[e.target].s.role === 'code'); });
      h += finding('Static edges never observed', neverObserved.length, esc(neverObserved.slice(0, 5).map(function (e) { return e.source + ' → ' + e.target; }).join(', ')) + (neverObserved.length > 5 ? ', …' : ''));
    }
    return h;
  }
  function showChange() {
    var c = B.change, h = '';
    var kind = c.kind === 'pr' ? 'PR ' + c.reference : c.kind === 'diff' ? 'diff ' + c.reference : c.kind === 'commit' ? 'commit ' + c.reference : c.reference + ' given';
    h += '<p class="name">' + esc(kind) + '</p>';
    h += '<dl class="kv">';
    if (c.how) h += '<dt>found as</dt><dd>' + esc(c.how) + '</dd>';
    if (c.title) h += '<dt>title</dt><dd>' + esc(c.title) + '</dd>';
    if (c.date) h += '<dt>date</dt><dd>' + esc(c.date) + '</dd>';
    h += '<dt>depth</dt><dd>' + B.depth + '</dd>';
    h += '<dt>files</dt><dd>' + c.files.length + (c.outsideRoot ? ' (+' + c.outsideRoot + ' outside the analysed directory)' : '') + '</dd>';
    h += '</dl>';
    var by = B.summary.byConfidence, parts = [];
    ['observed', 'static', 'inferred', 'uncertain'].forEach(function (k) { if (by[k]) parts.push(by[k] + ' ' + k); });
    h += '<p class="fixed"><strong>' + plural(B.summary.changed, 'service', 'services') + ' changed → ' + plural(B.summary.reached, 'service', 'services') + ' in the blast radius</strong>' + (parts.length ? '<br><span class="hint">' + parts.join(' · ') + '</span>' : '') + '</p>';
    h += '<h2>Changed · ' + B.changed.length + '</h2>';
    if (B.changed.length) h += '<div class="list">' + B.changed.map(function (x) { return '<strong style="color:var(--text)">' + esc(x.service) + '</strong> · ' + plural(x.files.length, 'file', 'files') + '<br>' + esc(x.files.slice(0, 5).join(', ')) + (x.files.length > 5 ? ', …' : ''); }).join('<br>') + '</div>';
    else h += '<p class="hint">None of the changed files belongs to a discovered service.</p>';
    if (B.unowned.length) h += '<p class="hint">' + plural(B.unowned.length, 'file belongs', 'files belong') + ' to no service: ' + esc(B.unowned.slice(0, 8).join(', ')) + (B.unowned.length > 8 ? ', …' : '') + '</p>';
    h += '<h2>Reached · ' + B.reached.length + '</h2>';
    if (B.reached.length) h += '<table class="reached">' + B.reached.map(function (r) { return '<tr><td class="svc"><strong>' + esc(r.service) + '</strong><small>depth ' + r.depth + '</small></td><td class="conf">' + badge(r.confidence) + '</td><td class="path">' + pathWords(r.path) + '</td></tr>'; }).join('') + '</table>';
    else h += '<p class="hint">No static or observed path leads out of the changed services.</p>';
    if (B.infrastructure.length) h += '<h2>Infrastructure on the path</h2><p class="hint">' + B.infrastructure.map(function (t) { return esc(t.service) + ' (published to by ' + esc(t.via) + ')'; }).join(', ') + '. Every other client of such a broker is included as uncertain.</p>';
    h += '<h2>Not reached · ' + B.notReached.length + '</h2><p class="fixed hint">' + esc(payload.notReached) + '</p>';
    if (B.notReached.length) h += '<div class="list">' + esc(B.notReached.join(', ')) + '</div>';
    return h;
  }
  function showDefault() {
    if (!nodes.length) { panel.innerHTML = '<p class="hint">No services were discovered in this repository, so there is nothing to draw.</p>'; return; }
    panel.innerHTML = (B ? showChange() : '<p class="hint">Click a node for its declaration, its edges and their evidence.</p>') + showFindings();
  }
  showDefault();
})();
</script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_cannot_close_the_script_element() {
        let out = escape_json(r#"{"name":"</script><script>alert(1)</script>&"}"#);
        assert!(!out.contains('<') && !out.contains('>') && !out.contains('&'));
        assert!(out.contains("\\u003c/script\\u003e"));
        assert_eq!(escape_html("a<b>&\"c\""), "a&lt;b&gt;&amp;&quot;c&quot;");
    }
}
