// Prints the text of every line box, in document order, as JSON.
document.fonts.ready.then(() => {
  const lines = [];
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (!node.data.trim()) continue;
    let current = null, top = null;
    for (let i = 0; i < node.data.length; i++) {
      const range = document.createRange();
      range.setStart(node, i); range.setEnd(node, i + 1);
      const rect = range.getBoundingClientRect();
      if (rect.height === 0) continue;
      if (top === null || Math.abs(rect.top - top) > 2) {
        if (current !== null) lines.push(current);
        current = ''; top = rect.top;
      }
      current += node.data[i];
    }
    if (current !== null) lines.push(current);
  }
  const out = document.createElement('pre');
  out.id = 'lines';
  out.textContent = JSON.stringify(lines.map(l => l.replace(/\s+/g, ' ').trim()).filter(l => l));
  document.body.appendChild(out);
});
