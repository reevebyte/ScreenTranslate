const DEFAULT_ACCENTS = new Set(['', '#18181b', '#15803d', '#303030']);

function luminance(hex: string) {
  const channels = hex.slice(1).match(/../g)!.map(value => {
    const channel = parseInt(value, 16) / 255;
    return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
  });
  return channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722;
}

export function contrastRatio(first: string, second: string) {
  const a = luminance(first), b = luminance(second);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

export function actionPalette(accent: string, dark: boolean) {
  const primary = DEFAULT_ACCENTS.has(accent.toLowerCase()) || !/^#[a-f\d]{6}$/i.test(accent)
    ? (dark ? '#f0f0f0' : '#303030') : accent;
  const foreground = contrastRatio(primary, '#ffffff') >= 4.5 ? '#ffffff' : '#000000';
  const surface = dark ? '#1a1a1a' : '#ffffff';
  const focus = contrastRatio(primary, surface) >= 3 ? primary : (dark ? '#bdbdbd' : '#242424');
  return { primary, foreground, focus };
}
