const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

export function humanBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  let n = bytes;
  let i = 0;
  while (n >= 1024 && i < UNITS.length - 1) {
    n /= 1024;
    i += 1;
  }
  return `${n.toFixed(1)} ${UNITS[i]}`;
}

export function Bytes({ n }: { n: number }) {
  return <span className="font-mono text-text-muted">{humanBytes(n)}</span>;
}
