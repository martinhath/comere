#!/bin/awk -f
# NOTE the number of entries here is the number of column in the output, not the number of files.
{
  a[FNR] = a[FNR]" "$1;
  b[FNR] = b[FNR]" "$2;
  c[FNR] = c[FNR]" "$3;
  d[FNR] = d[FNR]" "$4;
  e[FNR] = e[FNR]" "$5;
}

END {
  for (i in a) { printf("%s\n",  a[i]) > "col-1"; }
  for (i in b) { printf("%s\n",  b[i]) > "col-2"; }
  for (i in c) { printf("%s\n",  c[i]) > "col-3"; }
  for (i in d) { printf("%s\n",  d[i]) > "col-4"; }
  for (i in e) { printf("%s\n",  e[i]) > "col-5"; }
}
