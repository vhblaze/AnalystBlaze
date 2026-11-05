/** Mesma forma do objeto em português, com qualquer texto: garante as mesmas chaves em en-US. */
export type Mirror<T> = { [K in keyof T]: T[K] extends string ? string : Mirror<T[K]> };
