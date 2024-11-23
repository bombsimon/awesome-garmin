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
    sorted=$(echo "$rows" | sort)

    if [ "$rows" != "$sorted" ]; then
        invalid_sections+=("$header")
        exit_code=1
    fi
done

if [ $exit_code -ne 0 ]; then
    cat <<-EOF
Thanks for adding new resources to this project!

To help with consistency and easie maintenance, e.g. spot duplicates the items in each section is sorted alphanumerically

Your change doesn't conform to this so please ensure the section(s) you've edited are sorted!

These sections are currently not sorted:

EOF

    for s in "${invalid_sections[@]}"; do
        echo " - \\\`$s\\\`"
    done
fi

exit $exit_code
