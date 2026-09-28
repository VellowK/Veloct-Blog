// 作品本体脚本。
//
// 它运行在独立的 Works Origin 上，而不是主站同源。主站只通过带 sandbox 的 iframe 嵌入它，
// 且不授予 allow-same-origin，所以这里刻意不使用任何存储 API：即使写出来，也无法访问主站的
// 会话或 LocalStorage。
document.addEventListener('DOMContentLoaded', () => {
  const output = document.getElementById('count');
  const button = document.getElementById('increment');
  if (!output || !button) return;

  let count = 0;
  button.addEventListener('click', () => {
    count += 1;
    output.textContent = String(count);
  });
});
