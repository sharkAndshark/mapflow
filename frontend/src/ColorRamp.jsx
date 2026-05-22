const RAMPS = {
  categorical: [
    {
      name: 'Set1',
      colors: [
        '#e41a1c',
        '#377eb8',
        '#4daf4a',
        '#984ea3',
        '#ff7f00',
        '#ffff33',
        '#a65628',
        '#f781bf',
      ],
    },
    {
      name: 'Set2',
      colors: [
        '#66c2a5',
        '#fc8d62',
        '#8da0cb',
        '#e78ac3',
        '#a6d854',
        '#ffd92f',
        '#e5c494',
        '#b3b3b3',
      ],
    },
    {
      name: 'Set3',
      colors: [
        '#8dd3c7',
        '#ffffb3',
        '#bebada',
        '#fb8072',
        '#80b1d3',
        '#fdb462',
        '#b3de69',
        '#fccde5',
      ],
    },
    {
      name: 'Paired',
      colors: [
        '#a6cee3',
        '#1f78b4',
        '#b2df8a',
        '#33a02c',
        '#fb9a99',
        '#e31a1c',
        '#fdbf6f',
        '#ff7f00',
      ],
    },
    {
      name: 'Accent',
      colors: [
        '#7fc97f',
        '#beaed4',
        '#fdc086',
        '#ffff99',
        '#386cb0',
        '#f0027f',
        '#bf5b17',
        '#666666',
      ],
    },
    {
      name: 'Dark2',
      colors: [
        '#1b9e77',
        '#d95f02',
        '#7570b3',
        '#e7298a',
        '#66a61e',
        '#e6ab02',
        '#a6761d',
        '#666666',
      ],
    },
    {
      name: 'Pastel1',
      colors: [
        '#fbb4ae',
        '#b3cde3',
        '#ccebc5',
        '#decbe4',
        '#fed9a6',
        '#ffffcc',
        '#e5d8bd',
        '#fddaec',
      ],
    },
    {
      name: 'Pastel2',
      colors: [
        '#b3e2cd',
        '#fdcdac',
        '#cbd5e8',
        '#f4cae4',
        '#e6f5c9',
        '#fff2ae',
        '#f1e2cc',
        '#cccccc',
      ],
    },
  ],
  sequential: [
    { name: 'Blues', colors: ['#deebf7', '#9ecae1', '#3182bd'] },
    { name: 'Reds', colors: ['#fee0d2', '#fc9272', '#de2d26'] },
    { name: 'Greens', colors: ['#e5f5e0', '#a1d99b', '#31a354'] },
    { name: 'Oranges', colors: ['#fee6ce', '#fdae6b', '#e6550d'] },
    { name: 'Purples', colors: ['#efedf5', '#bcbddc', '#756bb1'] },
    { name: 'Greys', colors: ['#f0f0f0', '#bdbdbd', '#636363'] },
    { name: 'YlOrRd', colors: ['#ffffb2', '#fecc5c', '#fd8d3c', '#e31a1c'] },
    { name: 'YlGnBu', colors: ['#ffffcc', '#a1dab4', '#41b6c4', '#253494'] },
    { name: 'YlGn', colors: ['#ffffcc', '#addd8e', '#41ab5d'] },
    { name: 'BuPu', colors: ['#edf8fb', '#8c96c6', '#810f7c'] },
    { name: 'GnBu', colors: ['#f0f9e8', '#7bccc4', '#084081'] },
    { name: 'PuBu', colors: ['#f1eef6', '#80b1d3', '#2b8cbe'] },
    { name: 'BuGn', colors: ['#edf8fb', '#7fcdbb', '#005824'] },
    { name: 'PuRd', colors: ['#f1eef6', '#df65b0', '#67001f'] },
    { name: 'OrRd', colors: ['#fef0d9', '#fc8d59', '#b30000'] },
    { name: 'RdPu', colors: ['#fef0d9', '#dd1c77', '#980043'] },
    { name: 'Viridis', colors: ['#440154', '#31688e', '#35b779', '#fde725'] },
    { name: 'Inferno', colors: ['#160b39', '#b73779', '#fc8961', '#fcfdbf'] },
    { name: 'Magma', colors: ['#000004', '#b73779', '#fc8961', '#fcfdbf'] },
    { name: 'Plasma', colors: ['#0d0887', '#cc4778', '#f89540', '#f0f921'] },
  ],
  diverging: [
    { name: 'RdBu', colors: ['#67001f', '#f7f7f7', '#053061'] },
    { name: 'RdYlGn', colors: ['#a50026', '#ffffbf', '#006837'] },
    { name: 'RdYlBu', colors: ['#a50026', '#ffffbf', '#313695'] },
    { name: 'Spectral', colors: ['#9e0142', '#ffffbf', '#5e4fa2'] },
    { name: 'BrBG', colors: ['#543005', '#f5f5f5', '#003c30'] },
    { name: 'PiYG', colors: ['#8e0152', '#f7f7f7', '#276419'] },
    { name: 'PRGn', colors: ['#762a83', '#f7f7f7', '#1b7837'] },
    { name: 'PuOr', colors: ['#7b3294', '#f7f7f7', '#276419'] },
    { name: 'RdGy', colors: ['#67001f', '#ffffff', '#1a1a1a'] },
  ],
};

export default function ColorRampSelector({ mode, value, onChange }) {
  let ramps;
  if (mode === 'categorical') {
    ramps = RAMPS.categorical;
  } else if (mode === 'diverging') {
    ramps = RAMPS.diverging;
  } else {
    ramps = [...RAMPS.sequential, ...RAMPS.diverging];
  }

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
            <div
              style={{
                display: 'flex',
                height: '14px',
                borderRadius: '2px',
                overflow: 'hidden',
                flex: 1,
              }}
            >
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
  const all = [...RAMPS.categorical, ...RAMPS.sequential, ...RAMPS.diverging];
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
