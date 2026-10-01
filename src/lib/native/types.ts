/** JSON wire representation, shared by presentation components and the Rust transport. */
export type Wire<T> = T extends Date
	? string
	: T extends Array<infer U>
		? Wire<U>[]
		: T extends object
			? { [K in keyof T]: Wire<T[K]> }
			: T;
