/* Extract from ifduyue/musl src/ctype/isspace.c at
 * commit 7813e9119eb5fe681b6750b8a033a0490bca0941
 * upstream blob 428813e79856c9092942e26dfd83eb94926c65ea. Original function declaration/body preserved.
 * The surrounding headers and locale wrappers have been omitted.
 * musl COPYRIGHT (MIT): see ../UPSTREAM-LICENSE.txt.
 */
int isspace(int c)
{
	return c == ' ' || (unsigned)c-'\t' < 5;
}
