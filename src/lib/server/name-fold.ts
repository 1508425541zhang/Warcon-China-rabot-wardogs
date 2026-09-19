// How the Name filter reads a name: folded so that case, accents, look-alike letters from other
// alphabets and spelled-out letters are gone, then searched with a pattern per blocked word that
// allows leetspeak and stretched letters. No dependency, and nothing an admin types is ever run as
// a pattern: a word's pattern is built from its own letters and digits.

// Letters of other alphabets that pass for a Latin one. A blocked word is folded the same way as
// a name, so a Cyrillic word still finds itself.
const LOOKALIKES: Record<string, string> = {
	а: 'a',
	в: 'b',
	е: 'e',
	ё: 'e',
	к: 'k',
	м: 'm',
	н: 'h',
	о: 'o',
	р: 'p',
	с: 'c',
	т: 't',
	у: 'y',
	х: 'x',
	і: 'i',
	ї: 'i',
	ј: 'j',
	ѕ: 's',
	ԁ: 'd',
	ɡ: 'g',
	α: 'a',
	β: 'b',
	ε: 'e',
	η: 'n',
	ι: 'i',
	κ: 'k',
	ν: 'v',
	ο: 'o',
	ρ: 'p',
	τ: 't',
	υ: 'u',
	χ: 'x',
	ø: 'o',
	đ: 'd',
	ł: 'l',
	ß: 'ss',
	æ: 'ae',
	œ: 'oe'
};

// Leetspeak, read back: what stands in for a letter becomes the letter. "1" and "|" serve for both
// i and l, so l reads as i too (in the word as in the name). Every stand-in has one reading, so a
// word's pattern never has two neighbours that accept the same character, and a hostile name
// cannot make it backtrack.
const LEET: Record<string, string> = {
	'4': 'a',
	'@': 'a',
	'8': 'b',
	'3': 'e',
	'9': 'g',
	'1': 'i',
	'!': 'i',
	'|': 'i',
	l: 'i',
	'0': 'o',
	'5': 's',
	$: 's',
	'7': 't',
	'+': 't',
	'2': 'z'
};

/** Longer than any name the game allows; bounds the work a hostile name can ask for. */
const MAX_NAME = 100;

// "n.a.z.i" and "N A Z I" read as one word: single characters strung together by separators are
// joined before the words are looked for. "Dick Van Dyke" is three words and stays three.
const SPELLED = /(?<![\p{L}\p{N}])[\p{L}\p{N}](?:[^\p{L}\p{N}]+[\p{L}\p{N}](?![\p{L}\p{N}]))+/gu;

/**
 * A name or a word as the matcher sees it: compatibility forms, case, accents and look-alike
 * letters gone, spelled-out letters joined, and with `leet` the leetspeak read back as letters.
 */
export function fold(text: string, leet = false): string {
	const plain = text
		.slice(0, MAX_NAME)
		.normalize('NFKC')
		.toLowerCase()
		.normalize('NFD')
		.replace(/\p{M}/gu, '');
	let out = '';
	for (const raw of plain) {
		const ch = LOOKALIKES[raw] ?? raw;
		out += leet ? (LEET[ch] ?? ch) : ch;
	}
	return out.replace(SPELLED, (run) => run.replace(/[^\p{L}\p{N}]+/gu, ''));
}

export interface WordPattern {
	/** which reading of a name it is looked for in: a word with a digit in it means the digit */
	leet: boolean;
	pattern: RegExp;
}

/**
 * The pattern that finds one blocked word in a folded name: each run of a letter may be stretched
 * ("naaazi", and "kkk" needs its three) and a space in the word is any separators or none. Built
 * from the word's own letters and digits, one literal per run, so it matches in linear time and
 * nothing an admin types is run as a pattern.
 */
export function wordPattern(word: string): WordPattern {
	const leet = !/\p{N}/u.test(word);
	// A space between two of the same letter would let both runs claim the same characters.
	const chars = [...fold(word, leet)]
		.filter((ch) => /[\p{L}\p{N} ]/u.test(ch))
		.filter((ch, i, all) => ch !== ' ' || all[i - 1] !== all[i + 1]);
	let src = '';
	for (let i = 0; i < chars.length;) {
		let n = 1;
		while (chars[i + n] === chars[i]) n++;
		src +=
			chars[i] === ' '
				? '[^\\p{L}\\p{N}]*'
				: `${chars[i].replace(/[^\p{L}\p{N}]/gu, '')}${n === 1 ? '+' : `{${n},}`}`;
		i += n;
	}
	return { leet, pattern: new RegExp(src, 'u') };
}
