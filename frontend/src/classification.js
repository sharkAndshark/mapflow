export function equalIntervalBreaks(min, max, count) {
  const step = (max - min) / count;
  const breaks = [];
  for (let i = 1; i <= count; i++) {
    breaks.push(min + step * i);
  }
  return breaks;
}

export function quantileBreaks(sortedValues, count) {
  if (!sortedValues || sortedValues.length === 0) return [];
  const n = sortedValues.length;
  const breaks = [];
  for (let i = 1; i <= count; i++) {
    const pos = (n * i) / (count + 1);
    const idx = Math.floor(pos) - 1;
    const frac = pos - Math.floor(pos);
    const lo = sortedValues[Math.max(0, Math.min(n - 1, idx))];
    const hi = sortedValues[Math.max(0, Math.min(n - 1, idx + 1))];
    breaks.push(lo + frac * (hi - lo));
  }
  return breaks;
}

export function jenksBreaks(sortedValues, count) {
  if (!sortedValues || sortedValues.length === 0) return [];
  const n = sortedValues.length;
  if (n <= count) return equalIntervalBreaks(sortedValues[0], sortedValues[n - 1], count);

  const k = count;

  const lowerClassLimits = Array.from({ length: n + 1 }, () => Array(k + 1).fill(Infinity));
  lowerClassLimits[0][0] = 0;

  for (let i = 1; i <= n; i++) {
    let sum = 0;
    let sumSquares = 0;
    for (let j = 1; j <= i; j++) {
      sum += sortedValues[j - 1];
      sumSquares += sortedValues[j - 1] * sortedValues[j - 1];
      const variance = sumSquares - (sum * sum) / j;

      if (j > 1) {
        for (let l = 2; l <= k; l++) {
          if (lowerClassLimits[i][l] >= variance + lowerClassLimits[j - 1][l - 1]) {
            lowerClassLimits[i][l] = variance + lowerClassLimits[j - 1][l - 1];
          }
        }
      } else {
        lowerClassLimits[i][1] = variance;
      }
    }
  }

  const breaks = [];
  let kval = n;
  let klass = k;
  while (klass > 0) {
    let prevK = kval - 1;
    let found = false;
    const variance = lowerClassLimits[n][k];
    for (let j = 0; j < kval; j++) {
      const sum = sortedValues.slice(j, kval).reduce((a, b) => a + b, 0);
      const mean = sum / (kval - j);
      const ss = sortedValues.slice(j, kval).reduce((a, b) => a + (b - mean) ** 2, 0);
      if (Math.abs(variance - ss - lowerClassLimits[j][klass - 1]) < 1e-10) {
        prevK = j;
        found = true;
        break;
      }
    }

    breaks.unshift(sortedValues[kval - 1]);
    if (klass === 1) break;
    klass--;
    kval = prevK;
    if (kval <= 0) break;
  }

  return breaks.slice(0, k);
}
