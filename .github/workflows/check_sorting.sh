#!/usr/bin/env bash

file="awesome-generator/awesome.toml"
exit_code=0

IFS=";"

# shellcheck disable=SC2207
# This just feels like the easiest to understand. I don't know how to split the
# file on multiple newlines any better way.
# Ref: https://stackoverflow.com/a/62608718/2274551
sections=($(awk -v RS= -v ORS=";" '{print}' "$file"))

for section in "${sections[@]}"; do
    header=$(echo "$section" | head -n 1)
    rows=$(echo "$section" | tail -n +2)
    sorted=$(echo "$rows" | LC_ALL=C sort)

    if [ "$rows" != "$sorted" ]; then
        invalid_sections+=("$header")
        exit_code=1
    fi
done

if [ $exit_code -ne 0 ]; then
    cat <<-EOF
Thank you for adding new resources to this project!

To ensure consistency and easier maintenance (e.g., spotting duplicates), the
items in each section are sorted alphanumerically.

Your recent changes don't follow this convention, so please ensure the
section(s) you've edited are properly sorted.

**NOTE** Sorting is case sensitive and since all uppercase letters comes before
lowercase letters, ensure the whole section is sorted. This means that 'B' comes
before 'a' and 'b' comes after 'C'.

The following sections are currently not sorted:

EOF

    for s in "${invalid_sections[@]}"; do
        echo " - $s" | tr -d "[]"
    done
fi

exit $exit_code
