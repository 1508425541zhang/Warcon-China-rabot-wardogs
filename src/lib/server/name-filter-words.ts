// The Name filter's built-in list: slurs and hate terms, the names an admin would remove from any
// server. Ordinary swearing is not here; an admin who minds it adds the words to the rule. A word
// matches anywhere in a name and its letters may be stretched, so each one that sits inside
// innocent words carries them as exceptions, and a word too short to tell apart ("rape" is in
// wrapped and rapper, "spic" in aspic, "fag" in leafage) is left to the admin. Keep it short and
// unambiguous: a wrong kick is worse than a missed name. name-filter.test.ts runs the list against
// the system dictionary where there is one; a new word goes through that before it lands.
export const BUILTIN_WORDS: { word: string; except?: string[] }[] = [
	{ word: 'nigger', except: ['snigger'] },
	{ word: 'nigga', except: ['niggard'] },
	{ word: 'niglet' },
	{ word: 'coon', except: ['raccoon', 'racoon', 'cocoon', 'tycoon', 'cooney', 'puccoon'] },
	{ word: 'chink' },
	{ word: 'gook', except: ['gobbledygook'] },
	{ word: 'wetback' },
	{ word: 'beaner', except: ['beanery'] },
	{ word: 'paki', except: ['pakistan'] },
	{ word: 'towelhead' },
	{ word: 'raghead' },
	{ word: 'faggot' },
	{ word: 'fagot', except: ['fagott'] },
	{ word: 'dyke', except: ['vandyke', 'van dyke'] },
	{ word: 'tranny' },
	{ word: 'shemale' },
	{ word: 'retard', except: ['retardant'] },
	{ word: 'mongoloid' },
	{ word: 'nazi', except: ['ashkenaz', 'nazir', 'nazim'] },
	{ word: 'hitler', except: ['whittle'] },
	{ word: 'sieg heil' },
	{ word: 'fuhrer' },
	{ word: 'gestapo' },
	{ word: 'holocaust' },
	{ word: 'auschwitz' },
	// not "kkk": it is how Brazilian players write laughter
	{ word: 'ku klux' },
	{ word: 'klansman' },
	{ word: 'white power' },
	{ word: '1488' },
	{ word: 'rapist', except: ['therapist', 'trappist', 'rappist', 'serapist'] },
	{
		word: 'pedo',
		except: ['torpedo', 'speedo', 'pedolog', 'pedodont', 'pedomet', 'pedomorph', 'pedobapt']
	},
	{ word: 'paedo', except: ['paedolog', 'paedomet', 'paedomorph'] },
	{ word: 'molest' }
];
