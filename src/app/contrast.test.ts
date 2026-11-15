import { expect, test } from "vitest";
function luminance(hex: string) {
  const rgb = hex
    .match(/\w\w/g)!
    .map((value) => parseInt(value, 16) / 255)
    .map((value) =>
      value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4,
    );
  return rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
}
test("text, muted labels and editor tokens meet 4.5:1 on their surfaces", () => {
  for (const [text, background] of [
    ["e1e4ee", "1e2028"],
    ["a9afc0", "30394d"],
    ["c7d6f5", "30394d"],
    ["A6C69B", "272a35"],
    ["E5B695", "1e2028"],
    ["B6C5FF", "1e2028"],
    ["182039", "b6c5ff"],
  ]) {
    const a = luminance(text),
      b = luminance(background);
    expect(
      (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05),
    ).toBeGreaterThanOrEqual(4.5);
  }
});
