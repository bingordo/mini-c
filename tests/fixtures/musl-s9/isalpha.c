/* Extract from ifduyue/musl src/ctype/isalpha.c at
 * commit 7813e9119eb5fe681b6750b8a033a0490bca0941
 * upstream blob a87a93759869df31a40374efe3c9e10b3b21d4a2. Original function declaration/body preserved.
 * The surrounding headers and locale wrappers have been omitted.
 * musl COPYRIGHT (MIT): see ../UPSTREAM-LICENSE.txt.
 */
int isalpha(int c)
{
	return ((unsigned)c|32)-'a' < 26;
}
