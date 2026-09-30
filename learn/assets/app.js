// Học Rust qua open-xml-rust — mục lục, tiến độ, giao diện sáng/tối, nút copy.
// Mọi trang chỉ cần <body data-lesson="id"> và <main class="content">; phần còn lại do file này dựng.

const REPO = "https://github.com/AdonisGM/open-xml-rust/blob/main/";

// Toàn bộ lộ trình. `file` rỗng = bài chưa viết (hiện "sắp có").
const PARTS = [
  { title: "Bắt đầu", lessons: [
    { id: "home", num: "★", title: "Trang chủ & lộ trình", file: "index.html" },
  ]},
  { title: "Phần 0 · Khởi động", lessons: [
    { id: "p0-1", num: "0.1", title: "Office Open XML là gì?", file: "p0-1-openxml-la-gi.html" },
    { id: "p0-2", num: "0.2", title: "Cài Rust & bộ công cụ cargo", file: "p0-2-cong-cu.html" },
    { id: "p0-3", num: "0.3", title: "Bản đồ dự án", file: "p0-3-ban-do-du-an.html" },
  ]},
  { title: "Phần 1 · Cấu trúc dự án Rust", lessons: [
    { id: "p1-1", num: "1.1", title: "Cargo.toml & workspace", file: "p1-1-cargo-workspace.html" },
    { id: "p1-2", num: "1.2", title: "Crate, module & pub", file: "p1-2-crate-module.html" },
    { id: "p1-3", num: "1.3", title: "Kiến trúc phân tầng", file: "p1-3-kien-truc-phan-tang.html" },
  ]},
  { title: "Phần 2 · Tầng XML (openxml-xml)", lessons: [
    { id: "p2-1", num: "2.1", title: "XML namespace & kiểu Ns", file: "p2-1-namespace.html" },
    { id: "p2-2", num: "2.2", title: "macro_rules!: bảng namespace", file: "p2-2-macro.html" },
    { id: "p2-3", num: "2.3", title: "Xử lý lỗi: Error, From, ?", file: "p2-3-xu-ly-loi.html" },
    { id: "p2-4", num: "2.4", title: "Đọc XML: XmlReader, lifetime, Cow", file: "p2-4-xml-reader.html" },
    { id: "p2-5", num: "2.5", title: "Ghi XML: XmlWriter", file: "p2-5-xml-writer.html" },
    { id: "p2-6", num: "2.6", title: "Trait XmlValue & generic", file: "p2-6-xml-value.html" },
    { id: "p2-7", num: "2.7", title: "Raw node & round-trip không mất dữ liệu", file: "p2-7-raw-node.html" },
    { id: "p2-8", num: "2.8", title: "Tổng kết & bài tập lớn", file: "p2-8-tong-ket.html" },
  ]},
  { title: "Phần 3 · Đóng gói (openxml-opc)", lessons: [
    { id: "p3", num: "3.x", title: "ZIP, PartName, relationships", file: "" },
  ]},
  { title: "Phần 4 · Sinh code từ XSD", lessons: [
    { id: "p4", num: "4.x", title: "codegen → openxml-schema", file: "" },
  ]},
  { title: "Phần 5 · openxml-core", lessons: [
    { id: "p5", num: "5.x", title: "Units, part I/O, ảnh", file: "" },
  ]},
  { title: "Phần 6 · API tài liệu", lessons: [
    { id: "p6", num: "6.x", title: "docx, xlsx, pptx", file: "" },
  ]},
  { title: "Phần 7 · Chart, CLI & test", lessons: [
    { id: "p7", num: "7.x", title: "Biểu đồ, dòng lệnh, chiến lược test", file: "" },
  ]},
  { title: "Phần 8 · Tổng kết", lessons: [
    { id: "p8", num: "8.x", title: "Tự viết mini-openxml", file: "" },
  ]},
];

const ALL = PARTS.flatMap(p => p.lessons);
const READY = ALL.filter(l => l.file && l.id !== "home");
const STORE_KEY = "learn-oxr-done";
const THEME_KEY = "learn-oxr-theme";

function load(key, fallback) {
  try { const v = localStorage.getItem(key); return v == null ? fallback : JSON.parse(v); } catch { return fallback; }
}
function save(key, value) {
  try { localStorage.setItem(key, JSON.stringify(value)); } catch { /* bộ nhớ bị chặn: bỏ qua */ }
}

// Áp theme càng sớm càng tốt để tránh nháy màu.
(function applyTheme() {
  const t = load(THEME_KEY, null);
  if (t) document.documentElement.dataset.theme = t;
})();

function el(tag, attrs = {}, ...children) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") e.className = v; else if (k === "text") e.textContent = v; else e.setAttribute(k, v);
  }
  for (const c of children) if (c != null) e.append(c);
  return e;
}

function buildTopbar() {
  const bar = el("header", { class: "topbar" });
  const menu = el("button", { class: "icon-btn", id: "menu-btn", "aria-label": "Mở mục lục", text: "☰" });
  menu.onclick = () => document.body.classList.toggle("nav-open");
  const brand = el("a", { class: "brand", href: "index.html" });
  brand.innerHTML = "Học <span>Rust</span> qua open-xml-rust";
  const theme = el("button", { class: "icon-btn", "aria-label": "Đổi giao diện sáng/tối", title: "Sáng / tối" });
  const setIcon = () => {
    const dark = document.documentElement.dataset.theme === "dark" ||
      (!document.documentElement.dataset.theme && matchMedia("(prefers-color-scheme: dark)").matches);
    theme.textContent = dark ? "☀︎" : "☾";
  };
  theme.onclick = () => {
    const dark = theme.textContent === "☾";
    document.documentElement.dataset.theme = dark ? "dark" : "light";
    save(THEME_KEY, document.documentElement.dataset.theme);
    setIcon();
  };
  setIcon();
  const gh = el("a", { class: "icon-btn", href: REPO.replace("/blob/main/", ""), title: "Mã nguồn trên GitHub", text: "GitHub" });
  gh.style.cssText = "display:inline-grid;place-items:center;text-decoration:none;font-size:13px";
  bar.append(menu, brand, el("div", { class: "spacer" }), gh, theme);
  document.body.prepend(bar);
}

function buildSidebar(current) {
  const done = new Set(load(STORE_KEY, []));
  const nav = el("nav", { id: "sidebar", "aria-label": "Mục lục" });
  const count = READY.filter(l => done.has(l.id)).length;
  const prog = el("div", { class: "progress" }, `Đã học ${count}/${READY.length} bài`);
  const bar = el("div", { class: "bar" }, el("i"));
  bar.firstChild.style.width = `${READY.length ? (100 * count) / READY.length : 0}%`;
  prog.append(bar);
  nav.append(prog);
  for (const part of PARTS) {
    nav.append(el("h4", { text: part.title }));
    const ul = el("ul");
    for (const l of part.lessons) {
      const num = el("span", { class: "num", text: l.num });
      let item;
      if (l.file) {
        item = el("a", { href: l.file }, num, document.createTextNode(l.title));
        if (l.id === current) { item.classList.add("current"); item.setAttribute("aria-current", "page"); }
        if (done.has(l.id)) item.append(el("span", { class: "check", text: "✓" }));
      } else {
        item = el("span", {}, num, document.createTextNode(l.title), el("span", { class: "soon", text: "sắp có" }));
      }
      ul.append(el("li", {}, item));
    }
    nav.append(ul);
  }
  return nav;
}

function buildFooter(current, main) {
  const idx = READY.findIndex(l => l.id === current);
  if (idx < 0) return;
  const done = new Set(load(STORE_KEY, []));
  const foot = el("div", { class: "lesson-foot" });
  const btn = el("button", { class: "done-btn" });
  const paint = () => {
    const d = done.has(current);
    btn.textContent = d ? "✓ Đã học xong bài này" : "Đánh dấu đã học xong";
    btn.classList.toggle("is-done", d);
  };
  btn.onclick = () => {
    done.has(current) ? done.delete(current) : done.add(current);
    save(STORE_KEY, [...done]);
    paint();
    const old = document.getElementById("sidebar");
    old.replaceWith(buildSidebar(current));
  };
  paint();
  foot.append(btn);
  const pager = el("nav", { class: "pager", "aria-label": "Chuyển bài" });
  const prev = READY[idx - 1], next = READY[idx + 1];
  if (prev) pager.append(el("a", { href: prev.file, class: "prev" }, el("small", { text: "← Bài trước" }), `${prev.num} ${prev.title}`));
  if (next) pager.append(el("a", { href: next.file, class: "next" }, el("small", { text: "Bài tiếp →" }), `${next.num} ${next.title}`));
  foot.append(pager);
  main.append(foot);
}

function enhanceCode() {
  // Link "file:dòng" trong figcaption → GitHub, nếu tác giả chỉ ghi data-src.
  document.querySelectorAll("figcaption[data-src]").forEach(fc => {
    const src = fc.dataset.src;                // ví dụ "crates/openxml-xml/src/ns.rs#L124-L125"
    const [path] = src.split("#");
    const a = el("a", { href: REPO + src, target: "_blank", rel: "noopener", text: fc.textContent.trim() || path });
    fc.textContent = "";
    fc.append(a);
  });
  document.querySelectorAll("figure.code").forEach(fig => {
    const pre = fig.querySelector("pre");
    if (!pre || pre.classList.contains("diagram")) return;
    const btn = el("button", { class: "copy-btn", type: "button", text: "Copy" });
    btn.onclick = async () => {
      try { await navigator.clipboard.writeText(pre.innerText); btn.textContent = "Đã copy"; }
      catch { btn.textContent = "Không copy được"; }
      setTimeout(() => (btn.textContent = "Copy"), 1500);
    };
    fig.append(btn);
  });
  if (window.hljs) {
    document.querySelectorAll("pre code[class*='language-']").forEach(c => window.hljs.highlightElement(c));
  }
}

document.addEventListener("DOMContentLoaded", () => {
  const current = document.body.dataset.lesson || "";
  const main = document.querySelector("main.content");
  buildTopbar();
  const layout = el("div", { class: "layout" });
  main.replaceWith(layout);
  layout.append(buildSidebar(current), main);
  main.addEventListener("click", () => document.body.classList.remove("nav-open"));
  buildFooter(current, main);
  enhanceCode();
  const cur = document.querySelector("#sidebar a.current");
  const side = document.getElementById("sidebar");
  if (cur && side) side.scrollTop = cur.offsetTop - side.clientHeight / 2;
});
