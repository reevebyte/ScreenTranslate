let canvasContext: CanvasRenderingContext2D | null | undefined;

function context(): CanvasRenderingContext2D | null {
  if (canvasContext !== undefined) return canvasContext;
  if (typeof CanvasRenderingContext2D === 'undefined') return null;
  canvasContext = document.createElement('canvas').getContext('2d');
  return canvasContext;
}

/** Match the browser's font metrics instead of estimating CJK and Latin widths. */
export function sourceFontSize(block: { sourceLines?: { height: number }[]; fontSize?: number | null }) {
  const heights = block.sourceLines?.map(line => line.height).sort((a, b) => a - b);
  return heights?.length ? Math.min(32, Math.max(8, heights[Math.floor(heights.length / 2)] * 1.08)) : (block.fontSize || 13);
}

export function fitOverlayText(text: string, width: number, height: number, maximum: number) {
  const ctx = context();
  const family = getComputedStyle(document.body).fontFamily;
  const availableWidth = Math.max(1, width - 4);
  const availableHeight = Math.max(1, height - 2);
  const lineHeight = 1.2;
  const min = Math.min(8, maximum);
  let size = Math.max(min, maximum);
  if (!ctx) return { size, lineHeight, clipped: false };
  for (; size >= min; size = Math.max(min, size - 0.25)) {
    ctx.font = `${size}px ${family}`;
    let rows = 0;
    for (const paragraph of text.split('\n')) {
      let line = '';
      rows++;
      for (const token of paragraph.split(/(\s+)/)) {
        if (ctx.measureText(line + token).width <= availableWidth) { line += token; continue; }
        if (line.trim()) { rows++; line = ''; }
        const word = token.trimStart();
        if (ctx.measureText(word).width <= availableWidth) { line = word; continue; }
        for (const character of word) {
          if (line && ctx.measureText(line + character).width > availableWidth) { rows++; line = ''; }
          line += character;
        }
      }
    }
    const clipped = rows * size * lineHeight > availableHeight;
    if (!clipped || size === min) return { size, lineHeight, clipped };
  }
  return { size: min, lineHeight, clipped: true };
}
