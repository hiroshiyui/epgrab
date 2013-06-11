#include <stdio.h>
#include <string.h>

#include "tv_grab_dvb.h"

char *lookup(const struct lookup_table *l, int id) {
//	printf("Looked up %x", id);
	while ((l->u.i != id) && (l->u.i != -1))
		l++;
	return l->desc;
}

/* Read lookup_table from file into newly allocated table.
 * The table is a single allocation consisting of two parts:
 * first the array of structs, followed by a char-array of strings. */
int load_lookup(struct lookup_table **l, const char *file) {
	int name;
	char value[256];
	int n = 1, size = sizeof(struct lookup_table);

	if (file == NULL)
		return -1;

	FILE *fd = fopen(file, "r");
	if (!fd)
		return -1;

	// 1st: determine size needed
	while (fscanf(fd, "%d %255s", &name, value) == 2) {
		n++;
		size += sizeof(struct lookup_table);
		size += strlen(value) + 1;
	}
	struct lookup_table *p = *l = malloc(size);
	if (p == NULL)
		return -1;

	// 2nd: read data
	rewind(fd);
	char *c = (char *)(p + n);
	while (fscanf(fd, "%d %255s", &p->u.i, c) == 2) {
		p->desc = c;
		c += strlen(c) + 1;
		p++;
	}
	p->u.i = -1;
	p->desc = NULL;

	fclose(fd);
	return 0;
}

#ifdef MAIN
int main(int argc; char **argv;) {
	struct lookup_table *t;
	if (load_lookup(&t, "chanidents") != 0)
		return -1;
	if (argc > 1)
		printf("%s\n", lookup(t, atoi(argv[1])));
	return 0;
}
#endif
