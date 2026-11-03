/* Extract from ifduyue/musl src/ctype/isupper.c at
 * commit 7813e9119eb5fe681b6750b8a033a0490bca0941
 * upstream blob bfd15acdbe5291551a557d1a94980ae99e6e1024. Original function declaration/body preserved.
 * The surrounding headers and locale wrappers have been omitted.
 * musl COPYRIGHT (MIT): see ../UPSTREAM-LICENSE.txt.
 */
int isupper(int c)
{
	return (unsigned)c-'A' < 26;
}
