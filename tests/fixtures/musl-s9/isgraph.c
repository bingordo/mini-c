/* Extract from ifduyue/musl src/ctype/isgraph.c at
 * commit 7813e9119eb5fe681b6750b8a033a0490bca0941
 * upstream blob 292d1983ec5b717e66e2db62afacb078a267fbbf. Original function declaration/body preserved.
 * The surrounding headers and locale wrappers have been omitted.
 * musl COPYRIGHT (MIT): see ../UPSTREAM-LICENSE.txt.
 */
int isgraph(int c)
{
	return (unsigned)c-0x21 < 0x5e;
}
