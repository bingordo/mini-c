/* Extract from ifduyue/musl src/ctype/islower.c at
 * commit 7813e9119eb5fe681b6750b8a033a0490bca0941
 * upstream blob c3fa74c4cbd11896a370fe9565d0e4d1af16bf7b. Original function declaration/body preserved.
 * The surrounding headers and locale wrappers have been omitted.
 * musl COPYRIGHT (MIT): see ../UPSTREAM-LICENSE.txt.
 */
int islower(int c)
{
	return (unsigned)c-'a' < 26;
}
