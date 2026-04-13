const RAMPS = {
  categorical: [
    { name: 'Set1', colors: ['#e41a1c', '#377eb8', '#4daf4a', '#984ea3', '#ff7f00', '#ffff33', '#a65628', '#f781bf'] },
    { name: 'Pastel1', colors: ['#fbb4ae', '#b3cde3', '#ccebc5', '#decbe4', '#fed9a6', '#ffffcc', '#e5d8bd', '#fddaec'] },
    { name: 'Dark2', colors: ['#1b9e77', '#d95f02', '#7570b3', '#e7298a', '#66a61e', '#e6ab02', '#a6761d', '#666666'] },
  ],
  sequential: [
    { name: 'Blues', colors: ['#deebf7', '#9ecae1', '#3182bd'] },
    { name: 'Reds', colors: ['#fee0d2', '#fc9272', '#de2d26'] },
    { name: 'Greens', colors: ['#e5f5e0', '#a1d99b', '#31a354'] },
    { name: 'YlOrRd', colors: ['#ffffb2', '#fecc5c', '#fd8d3c', '#e31a1c'] },
    { name: 'Viridis', colors: ['#440154', '#31688e', '#35b779', '#fde725'] },
    { name: 'Inferno', colors: ['#160b39', '#b73779', '#fc8961', '#fcfdbf'] },
  ],
};

export default function ColorRampSelector({ mode, value, onChange }) {
  const ramps = mode === 'categorical' ? RAMPS.categorical : RAMPS.sequential;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
      {ramps.map((ramp) => {
        const isSelected = value === ramp.name;
        return (
          <button
            key={ramp.name}
            type="button"
            onClick={() => onChange(ramp.name)}
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: '6px',
              padding: '4px 6px',
              border: isSelected ? '2px solid #1976d2' : '1px solid #ddd',
              borderRadius: '4px',
              background: isSelected ? '#e3f2fd' : '#fff',
              cursor: 'pointer',
              fontSize: '11px',
            }}
          >
            <div style={{ display: 'flex', height: '14px', borderRadius: '2px', overflow: 'hidden', flex: 1 }}>
              {ramp.colors.map((c, i) => (
                <div key={i} style={{ flex: 1, backgroundColor: c }} />
              ))}
            </div>
            <span style={{ color: '#666', minWidth: '48px' }}>{ramp.name}</span>
          </button>
        );
      })}
    </div>
  );
}

export function resolveRampColors(rampName, count) {
  const all = [...RAMPS.categorical, ...RAMPS.sequential];
  const ramp = all.find((r) => r.name === rampName);
  if (!ramp) return interpolateColors(RAMPS.sequential[0].colors, count);
  if (count <= ramp.colors.length) return ramp.colors.slice(0, count);
  return interpolateColors(ramp.colors, count);
}

function interpolateColors(stops, count) {
  if (count <= 0) return [];
  if (count === 1) return [stops[Math.floor(stops.length / 2)]];
  const result = [];
  for (let i = 0; i < count; i++) {
    const t = i / (count - 1);
    const idx = t * (stops.length - 1);
    const lo = Math.floor(idx);
    const hi = Math.min(lo + 1, stops.length - 1);
    const frac = idx - lo;
    result.push(lerpColor(stops[lo], stops[hi], frac));
  }
  return result;
}

function lerpColor(a, b, t) {
  const ca = parseHex(a);
  const cb = parseHex(b);
  if (!ca || !cb) return a;
  const r = Math.round(ca.r + (cb.r - ca.r) * t);
  const g = Math.round(ca.g + (cb.g - ca.g) * t);
  const bl = Math.round(ca.b + (cb.b - ca.b) * t);
  return `#${r.toString(16).padStart(2, '0')}${g.toString(16).padStart(2, '0')}${bl.toString(16).padStart(2, '0')}`;
}

function parseHex(c) {
  const m = c.match(/^#([0-9a-f]{6})$/i);
  if (!m) return null;
  return {
    r: parseInt(m[1].substring(0, 2), 16),
    g: parseInt(m[1].substring(2, 4), 16),
    b: parseInt(m[1].substring(4, 6), 16),
  };
}
