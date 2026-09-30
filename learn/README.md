# learn/ — Học Rust qua open-xml-rust

Trang web tĩnh (HTML/CSS/JS thuần, không cần build) dạy Rust và Office Open XML
bằng cách đi lại toàn bộ dự án này từ đầu, bằng tiếng Việt.

## Xem trên máy

Mở `learn/index.html` trực tiếp bằng trình duyệt, hoặc chạy một web server nhỏ:

```bash
python3 -m http.server -d learn 8000   # rồi mở http://localhost:8000
```

## Đưa lên GitHub Pages

Workflow `.github/workflows/pages.yml` deploy thư mục `learn/` mỗi khi `main` thay đổi trong `learn/`.
Chỉ cần bật một lần: **Settings → Pages → Build and deployment → Source: GitHub Actions**,
rồi chạy workflow (tab Actions → "Deploy learn/ to GitHub Pages" → Run workflow).

## Cấu trúc

| File | Vai trò |
|------|---------|
| `index.html` | Trang chủ, lộ trình |
| `pX-Y-*.html` | Một bài học (Phần X, bài Y) |
| `assets/style.css` | Giao diện chung, sáng/tối |
| `assets/app.js` | Mục lục (mảng `PARTS`), tiến độ học (localStorage), nút copy, tô màu code |
| `assets/vendor/highlight.min.js` | highlight.js 11.9.0 (BSD-3-Clause), để trang chạy được cả khi offline |

Thêm bài mới: tạo file HTML theo khung của `p0-1-openxml-la-gi.html`, rồi khai báo `file` trong `PARTS` ở `app.js`.
