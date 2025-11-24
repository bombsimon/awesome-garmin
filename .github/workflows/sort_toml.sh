#!/usr/bin/env bash

# Sorts each section of awesome.toml alphanumerically
# This is needed after applying URL fixes that might break the sort order

file="awesome-generator/awesome.toml"
temp_file=$(mktemp)

# Process the file section by section
current_header=""
current_lines=""

while IFS= read -r line || [[ -n "$line" ]]; do
    # Check if this is a section header
    if [[ "$line" =~ ^\[.*\]$ ]]; then
        # If we have a previous section, write it out sorted
        if [ -n "$current_header" ]; then
            {
                echo "$current_header"
                echo "$current_lines" | LC_ALL=C sort
                echo ""
            } >>"$temp_file"
        fi

        current_header="$line"
        current_lines=""
    elif [ -n "$line" ]; then
        # Add non-empty lines to current section
        if [ -n "$current_lines" ]; then
            current_lines="$current_lines"$'\n'"$line"
        else
            current_lines="$line"
        fi
    fi
done <"$file"

# Don't forget the last section
if [ -n "$current_header" ]; then
    {
        echo "$current_header"
        echo "$current_lines" | LC_ALL=C sort
    } >>"$temp_file"
fi

mv "$temp_file" "$file"
echo "Sorted all sections in $file"
