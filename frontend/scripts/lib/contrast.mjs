/**
 * 计算 sRGB 颜色的 WCAG 2.x 相对亮度。
 * @param {[number, number, number]} rgb [r, g, b] 通道，取值 0-255
 * @returns {number} 相对亮度，取值 0-1
 */
export function relativeLuminance([r, g, b]) {
  const linearize = (c) => {
    const s = c / 255;
    return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  };
  return 0.2126 * linearize(r) + 0.7152 * linearize(g) + 0.0722 * linearize(b);
}

/**
 * 计算两种颜色之间的 WCAG 对比度。
 * @param {[number, number, number]} rgbA
 * @param {[number, number, number]} rgbB
 * @returns {number} 对比度，取值 1-21
 */
export function contrastRatio(rgbA, rgbB) {
  const l1 = relativeLuminance(rgbA);
  const l2 = relativeLuminance(rgbB);
  const max = Math.max(l1, l2);
  const min = Math.min(l1, l2);
  return (max + 0.05) / (min + 0.05);
}
