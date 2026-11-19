// SPDX-License-Identifier: MIT
export function storageValue(bytes: number): string {
  const units = ["B", "kB", "MB", "GB", "TB", "PB", "EB"];
  let unit = 0;
  while (bytes >= 1000 && unit < units.length - 1) { bytes /= 1000; unit++; }
  return `${new Intl.NumberFormat("en", { maximumSignificantDigits: 3 }).format(bytes)} ${units[unit]}`;
}
