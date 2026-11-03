/* Extract from ifduyue/musl src/ctype/isblank.c at
 * commit 7813e9119eb5fe681b6750b8a033a0490bca0941
 * upstream blob 716da23a566155ebc90646751a3be838523c0c10. Original function declaration/body preserved.
 * The surrounding headers and locale wrappers have been omitted.
 * musl COPYRIGHT (MIT): see ../UPSTREAM-LICENSE.txt.
 */
int isblank(int c)
{
	return (c == ' ' || c == '\t');
}
