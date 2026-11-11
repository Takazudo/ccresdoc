// Structural comparison for committed compatibility evidence. The check stays
// byte-for-byte strict; this module only explains a failure by naming the JSON
// paths that changed, with bounded old/new values.

const MISSING = Symbol("missing");

function isObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function childPath(path, key) {
  if (typeof key === "number") return `${path}[${key}]`;
  return /^[A-Za-z_$][\w$]*$/.test(key) ? `${path}.${key}` : `${path}[${JSON.stringify(key)}]`;
}

export function diffJson(before, after, { limit = 20 } = {}) {
  const differences = [];
  let total = 0;
  const visit = (path, left, right) => {
    if (Array.isArray(left) && Array.isArray(right)) {
      const length = Math.max(left.length, right.length);
      for (let index = 0; index < length; index += 1) {
        visit(childPath(path, index), index < left.length ? left[index] : MISSING, index < right.length ? right[index] : MISSING);
      }
      return;
    }
    if (isObject(left) && isObject(right)) {
      const keys = new Set([...Object.keys(left), ...Object.keys(right)]);
      for (const key of [...keys].sort()) {
        visit(childPath(path, key), Object.hasOwn(left, key) ? left[key] : MISSING, Object.hasOwn(right, key) ? right[key] : MISSING);
      }
      return;
    }
    if (Object.is(left, right)) return;
    total += 1;
    if (differences.length < limit) differences.push({ path: path || "$", old: left, new: right });
  };
  visit("$", before, after);
  return { differences, total };
}

function render(value, maxLength) {
  if (value === MISSING) return "<missing>";
  const text = JSON.stringify(value) ?? String(value);
  return text.length > maxLength ? `${text.slice(0, maxLength)}… (${text.length} chars)` : text;
}

export function formatEvidenceDrift(name, committedText, generatedValue, { limit = 20, maxValueLength = 200 } = {}) {
  let committed;
  try {
    committed = JSON.parse(committedText);
  } catch (error) {
    return `${name}: committed file is not valid JSON (${error.message})`;
  }
  // Compare the value as it serializes, so undefined, NaN and toJSON values
  // diff exactly as the byte comparison sees them.
  const generated = JSON.parse(JSON.stringify(generatedValue));
  const { differences, total } = diffJson(committed, generated, { limit });
  if (total === 0) return `${name}: same JSON value, different serialization (run evidence:refresh)`;
  const lines = [`${name}: ${total} changed value${total === 1 ? "" : "s"}`];
  for (const difference of differences) {
    lines.push(`  ${difference.path}: ${render(difference.old, maxValueLength)} -> ${render(difference.new, maxValueLength)}`);
  }
  if (total > differences.length) lines.push(`  … ${total - differences.length} more not shown`);
  return lines.join("\n");
}
