/* Extract from ifduyue/musl src/ctype/isdigit.c at
 * commit 7813e9119eb5fe681b6750b8a033a0490bca0941
 * upstream blob 16beddb4ca6a9ac7c1e2f47524cfe6c095d2485e. Original function declaration/body preserved.
 * The surrounding headers and locale wrappers have been omitted.
 * musl COPYRIGHT (MIT): see ../UPSTREAM-LICENSE.txt.
 */
int isdigit(int c)
{
	return (unsigned)c-'0' < 10;
}
