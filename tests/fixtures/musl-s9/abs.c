/* Extract from ifduyue/musl src/stdlib/abs.c at
 * commit 7813e9119eb5fe681b6750b8a033a0490bca0941
 * upstream blob e721fdc2b17e7917430eca885297b6e2d47986a1. Original function declaration/body preserved.
 * The surrounding headers and locale wrappers have been omitted.
 * musl COPYRIGHT (MIT): see ../UPSTREAM-LICENSE.txt.
 */
int abs(int a)
{
	return a>0 ? a : -a;
}
