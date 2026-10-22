import { highlight } from '../util';

/** Renders text with search terms wrapped in <mark>. Text nodes only, never HTML. */
export function Highlight({ text, terms }: { text: string; terms: string[] }) {
  return (
    <>
      {highlight(text, terms).map((s, i) => (s.match ? <mark key={i}>{s.text}</mark> : s.text))}
    </>
  );
}
