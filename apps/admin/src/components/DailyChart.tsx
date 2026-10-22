import { useRef, useState } from 'preact/hooks';
import type { DailyPoint } from '../types';
import { compact, niceMax } from '../util';

const H = 180;
const PAD = { top: 18, right: 8, bottom: 22, left: 34 };
const GAP = 2;
const RADIUS = 4;

/** Rect path with only the top two corners rounded (data end), square at the baseline. */
function topRounded(x: number, y: number, w: number, h: number, r: number): string {
  const rr = Math.min(r, h, w / 2);
  return `M${x},${y + h}V${y + rr}Q${x},${y} ${x + rr},${y}H${x + w - rr}Q${x + w},${y} ${x + w},${y + rr}V${y + h}Z`;
}

function shortDay(day: string): string {
  const [, m, d] = day.split('-');
  return `${Number(m)}/${Number(d)}`;
}

/** Weekly ticks plus the last day, skipping a weekly tick that would collide with it. */
export function showTick(i: number, n: number): boolean {
  if (i === n - 1) return true;
  return i % 7 === 0 && n - 1 - i >= 4;
}

/** Stacked daily columns: bugs (slot 1) at the baseline, feedback (slot 2) on top. */
export function DailyChart({ data, width = 720 }: { data: DailyPoint[]; width?: number }) {
  const [active, setActive] = useState<number | null>(null);
  const wrap = useRef<HTMLDivElement>(null);

  const plotW = width - PAD.left - PAD.right;
  const plotH = H - PAD.top - PAD.bottom;
  const max = niceMax(Math.max(0, ...data.map((d) => d.bug + d.feedback)));
  const band = data.length ? plotW / data.length : plotW;
  const barW = Math.min(24, Math.max(3, band * 0.62));
  const y = (v: number) => PAD.top + plotH - (v / max) * plotH;
  const ticks = [0, max / 2, max].filter((t, i, a) => a.indexOf(t) === i && Number.isInteger(t));

  let peak = -1;
  data.forEach((d, i) => {
    const t = d.bug + d.feedback;
    if (t > 0 && (peak < 0 || t >= data[peak]!.bug + data[peak]!.feedback)) peak = i;
  });

  const activePoint = active !== null ? data[active] : undefined;
  const tipX = active !== null ? ((PAD.left + band * active + band / 2) / width) * 100 : 0;
  const tipY = activePoint ? (y(activePoint.bug + activePoint.feedback) / H) * 100 : 0;

  return (
    <div class="chart" ref={wrap}>
      <div class="legend" aria-hidden="true">
        <span class="key">
          <span class="swatch bug" />
          Bugs
        </span>
        <span class="key">
          <span class="swatch feedback" />
          Feedback
        </span>
      </div>
      <svg
        viewBox={`0 0 ${width} ${H}`}
        role="img"
        aria-label="Submissions per day for the last 30 days, bugs and feedback stacked"
        onPointerLeave={() => setActive(null)}
      >
        {ticks.map((t) => (
          <g key={t}>
            <line class={t === 0 ? 'baseline' : 'grid-line'} x1={PAD.left} x2={width - PAD.right} y1={y(t)} y2={y(t)} />
            <text class="tick" x={PAD.left - 6} y={y(t) + 4} text-anchor="end">
              {compact(t)}
            </text>
          </g>
        ))}
        {data.map((d, i) => {
          const x = PAD.left + band * i + (band - barW) / 2;
          const bugH = (d.bug / max) * plotH;
          const fbH = (d.feedback / max) * plotH;
          const base = PAD.top + plotH;
          const bugTop = base - bugH;
          const fbBottom = d.bug > 0 ? bugTop - GAP : base;
          const fbTop = fbBottom - fbH;
          const label = `${d.day}: ${d.bug} bugs, ${d.feedback} feedback`;
          return (
            <g
              key={d.day}
              class={`col${active === i ? ' active' : ''}`}
              onPointerEnter={() => setActive(i)}
              onFocusIn={() => setActive(i)}
              onFocusOut={() => setActive(null)}
            >
              <rect
                class="hit"
                x={PAD.left + band * i}
                y={PAD.top}
                width={band}
                height={plotH}
                tabIndex={0}
                aria-label={label}
              />
              {d.bug > 0 && (
                <path
                  class="seg seg-bug"
                  d={d.feedback > 0 ? `M${x},${base}V${bugTop}H${x + barW}V${base}Z` : topRounded(x, bugTop, barW, bugH, RADIUS)}
                />
              )}
              {d.feedback > 0 && <path class="seg seg-feedback" d={topRounded(x, fbTop, barW, fbH, RADIUS)} />}
              {i === peak && (
                <text class="peak" x={x + barW / 2} y={Math.min(fbTop, bugTop) - 5} text-anchor="middle">
                  {compact(d.bug + d.feedback)}
                </text>
              )}
              {showTick(i, data.length) && (
                <text class="tick" x={PAD.left + band * i + band / 2} y={H - 6} text-anchor="middle">
                  {shortDay(d.day)}
                </text>
              )}
            </g>
          );
        })}
      </svg>
      {activePoint && (
        <div class="tooltip" style={{ left: `${tipX}%`, top: `${tipY}%` }} role="presentation">
          <div class="t-head">{activePoint.day}</div>
          <div class="t-row">
            <span class="linekey bug" />
            <strong>{activePoint.bug}</strong> bugs
          </div>
          <div class="t-row">
            <span class="linekey feedback" />
            <strong>{activePoint.feedback}</strong> feedback
          </div>
        </div>
      )}
      <details class="table-view">
        <summary>Show as table</summary>
        <div class="table-wrap">
          <table class="data">
            <thead>
              <tr>
                <th>Day</th>
                <th class="num">Bugs</th>
                <th class="num">Feedback</th>
                <th class="num">Total</th>
              </tr>
            </thead>
            <tbody>
              {data.map((d) => (
                <tr key={d.day}>
                  <td>{d.day}</td>
                  <td class="num">{d.bug}</td>
                  <td class="num">{d.feedback}</td>
                  <td class="num">{d.bug + d.feedback}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </details>
    </div>
  );
}
