# bareframe's zsh startup (ZDOTDIR points here). It runs Ghostty's integration, which
# restores the user's ZDOTDIR, reads the user's .zshenv and installs its own hooks; the
# user's .zprofile and .zshrc then load from their own ZDOTDIR as usual.
'builtin' 'source' '--' "$BAREFRAME_GHOSTTY_ZSH/.zshenv"
'builtin' 'unset' 'BAREFRAME_GHOSTTY_ZSH'

# A command bareframe offers at the first prompt, to run with Enter: picking up the agent
# that ran here when it quit (see src/resume.rs).
if [[ -n ${BAREFRAME_RESUME-} ]]; then
  _bareframe_resume() {
    'builtin' 'print' -z -- "$BAREFRAME_RESUME"
    'builtin' 'unset' 'BAREFRAME_RESUME'
    precmd_functions=(${precmd_functions:#_bareframe_resume})
    'builtin' 'unfunction' '_bareframe_resume'
  }
  precmd_functions+=(_bareframe_resume)
fi
