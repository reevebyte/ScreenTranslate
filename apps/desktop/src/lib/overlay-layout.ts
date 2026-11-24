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

export interface OverlayRenderBlock {
  text: string;
  translation: string;
  bounds: { x: number; y: number; width: number; height: number };
  foregroundColor?: string | null;
  sourceLines?: { height: number }[];
  fontSize?: number | null;
}

export interface OverlayTextLayout {
  size: number;
  lineHeight: number;
  clipped: boolean;
  lines: string[];
  family: string;
}

export function layoutOverlayText(text: string, width: number, height: number, maximum: number): OverlayTextLayout {
  const ctx = context();
  const family = getComputedStyle(document.body).fontFamily || 'sans-serif';
  const availableWidth = Math.max(1, width - 4);
  const availableHeight = Math.max(1, height - 2);
  const lineHeight = 1.2;
  const min = Math.min(8, maximum);
  let size = Math.max(min, maximum);
  if (!ctx) return { size, lineHeight, clipped: false, lines: text.split('\n'), family };
  for (; size >= min; size = Math.max(min, size - 0.25)) {
    ctx.font = `${size}px ${family}`;
    const lines: string[] = [];
    for (const paragraph of text.split('\n')) {
      let line = '';
      for (const token of paragraph.split(/(\s+)/)) {
        if (!token) continue;
        if (ctx.measureText(line + token).width <= availableWidth) { line += token; continue; }
        if (line.trim()) { lines.push(line); line = ''; }
        const word = token.trimStart();
        if (ctx.measureText(word).width <= availableWidth) { line = word; continue; }
        for (const character of word) {
          if (line && ctx.measureText(line + character).width > availableWidth) { lines.push(line); line = ''; }
          line += character;
        }
      }
      lines.push(line);
    }
    const clipped = lines.length * size * lineHeight > availableHeight;
    if (!clipped || size === min) return { size, lineHeight, clipped, lines, family };
  }
  return { size: min, lineHeight, clipped: true, lines: text.split('\n'), family };
}

export function fitOverlayText(text: string, width: number, height: number, maximum: number) {
  const { size, lineHeight, clipped } = layoutOverlayText(text, width, height, maximum);
  return { size, lineHeight, clipped };
}

function loadCanvasImage(url: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error('image-load-failed'));
    img.src = url;
  });
}

function canvasToPngBytes(canvas: HTMLCanvasElement): Promise<Uint8Array> {
  return new Promise((resolve, reject) => {
    if (typeof canvas.toBlob === 'function') {
      canvas.toBlob(async blob => {
        if (!blob) { reject(new Error('canvas-export-failed')); return; }
        resolve(new Uint8Array(await blob.arrayBuffer()));
      }, 'image/png');
      return;
    }
    try {
      const dataUrl = canvas.toDataURL('image/png');
      const base64 = dataUrl.split(',')[1] ?? '';
      const raw = atob(base64);
      const out = new Uint8Array(raw.length);
      for (let i = 0; i < raw.length; i++) out[i] = raw.charCodeAt(i);
      resolve(out);
    } catch (error) {
      reject(error);
    }
  });
}

/**
 * Render the current screenshot (either translated/edited composite or original)
 * at full native source pixel resolution using the same wrapping metrics as the preview.
 */
export async function renderTranslatedImagePng(
  imageUrl: string,
  width: number,
  height: number,
  blocks: OverlayRenderBlock[],
  drawBlocks: boolean,
): Promise<Uint8Array> {
  const img = await loadCanvasImage(imageUrl);
  const w = Math.max(1, Math.round(width || img.naturalWidth));
  const h = Math.max(1, Math.round(height || img.naturalHeight));
  const canvas = document.createElement('canvas');
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error('canvas-unavailable');
  ctx.drawImage(img, 0, 0, w, h);
  if (drawBlocks) {
    const fallbackColor = getComputedStyle(document.body).color || '#202127';
    ctx.textAlign = 'left';
    ctx.textBaseline = 'top';
    for (const block of blocks) {
      const content = block.translation || block.text;
      if (!content) continue;
      const fitted = layoutOverlayText(
        content,
        block.bounds.width,
        block.bounds.height,
        sourceFontSize(block),
      );
      ctx.save();
      ctx.beginPath();
      ctx.rect(block.bounds.x, block.bounds.y, block.bounds.width, block.bounds.height);
      ctx.clip();
      ctx.font = `${fitted.size}px ${fitted.family}`;
      ctx.fillStyle = block.foregroundColor || fallbackColor;
      const stride = fitted.size * fitted.lineHeight;
      const topOffset = (stride - fitted.size) / 2;
      fitted.lines.forEach((line, index) => {
        ctx.fillText(line, block.bounds.x + 2, block.bounds.y + 1 + topOffset + index * stride);
      });
      ctx.restore();
    }
  }
  return canvasToPngBytes(canvas);
}
