# bareframe's zsh startup (ZDOTDIR points here). Ghostty's integration runs first: it
# restores the user's ZDOTDIR, reads the user's .zshenv and installs its own hooks; the
# user's .zprofile and .zshrc then load from their own ZDOTDIR as usual.
'builtin' 'source' '--' "$BAREFRAME_GHOSTTY_ZSH/.zshenv"
'builtin' 'unset' 'BAREFRAME_GHOSTTY_ZSH'

# As in Warp, the prompt starts at the bottom of the screen and output scrolls up from
# there: before each prompt the cursor drops to the last row. Only at startup and after a
# clear is there room below it. Ctrl+L (which Cmd+K also sends) clears the same way.
if [[ -o 'interactive' ]]; then
    _bareframe_bottom() { 'builtin' 'print' -n -- $'\e[999B' }
    _bareframe_clear_screen() {
        'builtin' 'zle' -I
        'builtin' 'print' -n -- $'\e[H\e[2J\e[999B'
    }
    _bareframe_install() {
        'builtin' 'zle' -N clear-screen _bareframe_clear_screen
        precmd_functions=(${precmd_functions:#_bareframe_install})
    }
    'builtin' 'typeset' -ga precmd_functions
    precmd_functions=(_bareframe_bottom _bareframe_install $precmd_functions)
fi
