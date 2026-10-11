import { StreamLanguage } from '@codemirror/language';

const KEYWORDS = new Set([
  'break', 'case', 'centroid', 'const', 'continue', 'default', 'discard', 'do', 'else', 'flat', 'for', 'highp', 'if',
  'in', 'inout', 'invariant', 'layout', 'lowp', 'mediump', 'out', 'precision', 'return', 'smooth', 'struct', 'switch',
  'uniform', 'while',
]);
const TYPES = /^(void|bool|int|uint|float|[biu]?vec[234]|mat[234](x[234])?|[iu]?sampler(2D|3D|Cube|2DArray)(Shadow)?)$/;
const ATOMS = new Set(['true', 'false']);

/** GLSL ES highlighting good enough to read by: keywords, types, directives, numbers, comments. */
export const glsl = StreamLanguage.define<{ comment: boolean }>({
  name: 'glsl',
  startState: () => ({ comment: false }),
  token(stream, state) {
    if (state.comment) {
      if (stream.skipTo('*/')) {
        stream.match('*/');
        state.comment = false;
      } else stream.skipToEnd();
      return 'comment';
    }
    if (stream.sol() && stream.match(/^\s*#.*/)) return 'meta';
    if (stream.eatSpace()) return null;
    if (stream.match('//')) {
      stream.skipToEnd();
      return 'comment';
    }
    if (stream.match('/*')) {
      state.comment = true;
      return 'comment';
    }
    if (stream.match(/^(0[xX][0-9a-fA-F]+[uU]?|(\d+\.\d*|\.\d+|\d+)([eE][+-]?\d+)?[uUfF]?)/)) return 'number';
    const word = stream.match(/^[A-Za-z_]\w*/) as RegExpMatchArray | null;
    if (word) {
      const text = word[0];
      if (KEYWORDS.has(text)) return 'keyword';
      if (ATOMS.has(text)) return 'atom';
      if (TYPES.test(text)) return 'typeName';
      if (text.startsWith('gl_')) return 'variableName.special';
      if (stream.match(/^\s*\(/, false)) return 'variableName.function';
      return 'variableName';
    }
    stream.next();
    return null;
  },
  languageData: { commentTokens: { line: '//', block: { open: '/*', close: '*/' } } },
});
