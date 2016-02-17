use ignore::{Ignore, IgnoreGit, IgnoreVcs};
use pipeline::{PipelineFork, PipelineInfo};
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use time;

// ---------------------------------------------------------------------------------------------------------------------
// PathInfo
// ---------------------------------------------------------------------------------------------------------------------

#[derive(Debug,Clone)]
pub struct PathInfo {
    pub path: PathBuf,
    pub len : u64    ,
}

// ---------------------------------------------------------------------------------------------------------------------
// PipelineFinder
// ---------------------------------------------------------------------------------------------------------------------

pub struct PipelineFinder {
    pub is_recursive  : bool,
    pub follow_symlink: bool,
    pub skip_vcs      : bool,
    pub skip_gitignore: bool,
    pub skip_hgignore : bool,
    pub skip_ambignore: bool,
    pub print_skipped : bool,
    pub infos         : Vec<String>,
    pub errors        : Vec<String>,
    time_beg          : u64,
    time_end          : u64,
    time_bsy          : u64,
    msg_id            : usize,
    current_tx        : usize,
    ignore_vcs        : IgnoreVcs,
    ignore_git        : Vec<IgnoreGit>,
}

impl PipelineFinder {
    pub fn new() -> Self {
        PipelineFinder {
            is_recursive  : true,
            follow_symlink: true,
            skip_vcs      : true,
            skip_gitignore: true,
            skip_hgignore : true,
            skip_ambignore: true,
            print_skipped : false,
            infos         : Vec::new(),
            errors        : Vec::new(),
            time_beg      : 0,
            time_end      : 0,
            time_bsy      : 0,
            msg_id        : 0,
            current_tx    : 0,
            ignore_vcs    : IgnoreVcs::new(),
            ignore_git    : Vec::new(),
        }
    }

    fn find_path( &mut self, base: PathBuf, tx: &Vec<Sender<PipelineInfo<PathInfo>>> ) {

        let attr = match fs::metadata( &base ) {
            Ok ( x ) => x,
            Err( e ) => { self.errors.push( format!( "Error: {} @ {}", e, base.to_str().unwrap() ) ); return; },
        };

        if attr.is_file() {
            if attr.len() != 0 {
                self.send_path( base, attr.len(), &tx );
            }
        } else {
            let reader = match fs::read_dir( &base ) {
                Ok ( x ) => x,
                Err( e ) => { self.errors.push( format!( "Error: {} @ {}", e, base.to_str().unwrap() ) ); return; },
            };

            let gitignore_exist = self.push_gitignore( &base );

            for i in reader {
                match i {
                    Ok( entry ) => {
                        let file_type = match entry.file_type() {
                            Ok ( x ) => x,
                            Err( e ) => { self.errors.push( format!( "Error: {}", e ) ); continue },
                        };
                        if file_type.is_file() {
                            let metadata = match entry.metadata() {
                                Ok ( x ) => x,
                                Err( e ) => { self.errors.push( format!( "Error: {}", e ) ); continue },
                            };
                            if metadata.len() != 0 {
                                self.send_path( entry.path(), metadata.len(), &tx );
                            }
                        } else {
                            let find_dir     = file_type.is_dir()     & self.is_recursive;
                            let find_symlink = file_type.is_symlink() & self.is_recursive & self.follow_symlink;
                            if ( find_dir | find_symlink ) & self.check_dir( &entry.path() ) {
                                self.find_path( entry.path(), &tx );
                            }
                        }
                    },
                    Err( e ) => self.errors.push( format!( "Error: {}", e ) ),
                };
            }

            self.pop_gitignore( gitignore_exist )
        }
    }

    fn send_path( &mut self, path: PathBuf, len: u64, tx: &Vec<Sender<PipelineInfo<PathInfo>>> ) {
        if self.check_file( &path ) {
            let _ = tx[self.current_tx].send( PipelineInfo::Ok( self.msg_id, PathInfo{ path: path, len: len } ) );
            self.msg_id += 1;
            self.current_tx = if self.current_tx == tx.len() - 1 { 0 } else { self.current_tx + 1 };
        }
    }

    fn push_gitignore( &mut self, path: &PathBuf ) -> bool {
        if !self.skip_gitignore { return false }

        let mut reader = fs::read_dir( &path ).unwrap();
        let gitignore = reader.find( |x| {
            match x {
                &Ok ( ref x ) => x.path().ends_with( ".gitignore" ),
                &Err( _     ) => false,
            } 
        } );
        match gitignore {
            Some( Ok( x ) ) => { self.ignore_git.push( IgnoreGit::new( &x.path() ) ); true },
            _               => false,
        }
    }

    fn pop_gitignore( &mut self, exist: bool ) {
        if exist {
            let _ = self.ignore_git.pop();
        }
    }

    fn check_dir( &mut self, path: &PathBuf ) -> bool {
        let ok_vcs = if self.skip_vcs { self.ignore_vcs.check_dir( &path ) } else { true };
        let ok_git = if self.skip_gitignore && !self.ignore_git.is_empty() {
            self.ignore_git.last().unwrap().check_dir( &path )
        } else {
            true
        };

        if !ok_vcs & self.print_skipped {
            self.infos.push( format!( "Skipped: {:?} ( vcs file )\n", path ) );
        }

        if !ok_git & self.print_skipped {
            self.infos.push( format!( "Skipped: {:?} ( .gitignore )\n", path ) );
        }

        ok_vcs && ok_git
    }

    fn check_file( &mut self, path: &PathBuf ) -> bool {
        let ok_vcs = if self.skip_vcs { self.ignore_vcs.check_file( &path ) } else { true };
        let ok_git = if self.skip_gitignore && !self.ignore_git.is_empty() {
            self.ignore_git.last().unwrap().check_file( &path )
        } else {
            true
        };

        if !ok_vcs & self.print_skipped {
            self.infos.push( format!( "Skipped: {:?} ( vcs file )\n", path ) );
        }

        if !ok_git & self.print_skipped {
            self.infos.push( format!( "Skipped: {:?} ( .gitignore )\n", path ) );
        }

        ok_vcs && ok_git
    }

    fn set_default_gitignore( &mut self, base: &PathBuf ) {
        // TODO 

        //if !self.skip_gitignore { return }

        //let base_abs = match base.canonicalize() {
        //    Ok ( x ) => x,
        //    Err( e ) => { self.errors.push( format!( "Error: {} @ {}", e, base.to_str().unwrap() ) ); return; },
        //};

        //let mut parent = base_abs.parent();
        //println!( "{:?}", parent );
        //while parent.is_some() {
        //    if self.push_gitignore( &PathBuf::from( parent.unwrap() ) ) {
        //        break;
        //    }
        //    parent = parent.unwrap().parent();
        //    println!( "{:?}", parent );
        //}
    }
}

impl PipelineFork<PathBuf, PathInfo> for PipelineFinder {
    fn setup( &mut self, id: usize, rx: Receiver<PipelineInfo<PathBuf>>, tx: Vec<Sender<PipelineInfo<PathInfo>>> ) {
        loop {
            match rx.recv() {
                Ok( PipelineInfo::Ok( _, p ) ) => {
                    let beg = time::precise_time_ns();

                    self.set_default_gitignore( &p );
                    self.find_path( p, &tx );

                    let end = time::precise_time_ns();
                    self.time_bsy += end - beg;
                },

                Ok( PipelineInfo::Beg( x ) ) => {
                    self.msg_id = x;

                    self.infos  = Vec::new();
                    self.errors = Vec::new();

                    self.time_beg = time::precise_time_ns();

                    for tx in &tx {
                        let _ = tx.send( PipelineInfo::Beg( x ) );
                    }
                },

                Ok( PipelineInfo::End( _ ) ) => {
                    for i in &self.infos  { let _ = tx[0].send( PipelineInfo::Info( id, i.clone() ) ); }
                    for e in &self.errors { let _ = tx[0].send( PipelineInfo::Err ( id, e.clone() ) ); }

                    self.time_end = time::precise_time_ns();
                    let _ = tx[0].send( PipelineInfo::Time( id, self.time_bsy, self.time_end - self.time_beg ) );


                    for tx in &tx {
                        let _ = tx.send( PipelineInfo::End( self.msg_id ) );
                    }

                    break;
                },

                Ok ( PipelineInfo::Info( i, e      ) ) => { let _ = tx[0].send( PipelineInfo::Info( i, e      ) ); },
                Ok ( PipelineInfo::Err ( i, e      ) ) => { let _ = tx[0].send( PipelineInfo::Err ( i, e      ) ); },
                Ok ( PipelineInfo::Time( i, t0, t1 ) ) => { let _ = tx[0].send( PipelineInfo::Time( i, t0, t1 ) ); },
                Err( _                               ) => break,
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Test
// ---------------------------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::thread;
    use std::sync::mpsc;
    use pipeline::{PipelineFork, PipelineInfo};

    fn test<T: 'static+PipelineFork<PathBuf, PathInfo>+Send>( mut finder: T, path: String ) -> Vec<PathInfo> {
        let ( in_tx , in_rx  ) = mpsc::channel();
        let ( out_tx, out_rx ) = mpsc::channel();
        thread::spawn( move || {
            finder.setup( 0, in_rx, vec![out_tx] );
        } );
        let _ = in_tx.send( PipelineInfo::Beg( 0                        ) );
        let _ = in_tx.send( PipelineInfo::Ok ( 1, PathBuf::from( path ) ) );
        let _ = in_tx.send( PipelineInfo::End( 2                        ) );

        let mut ret = Vec::new();
        loop {
            match out_rx.recv().unwrap() {
                PipelineInfo::Ok  ( _, x ) => ret.push( x ),
                PipelineInfo::End ( _    ) => break,
                _                          => (),
            }
        }

        ret
    }

    #[test]
    fn pipeline_finder_default() {
        let finder = PipelineFinder::new();
        let ret = test( finder, "./".to_string() );

        assert!(  ret.iter().any( |x| x.path == PathBuf::from( "./Cargo.toml"     ) ) );
        assert!(  ret.iter().any( |x| x.path == PathBuf::from( "./src/ambr.rs"    ) ) );
        assert!(  ret.iter().any( |x| x.path == PathBuf::from( "./src/ambs.rs"    ) ) );
        assert!(  ret.iter().any( |x| x.path == PathBuf::from( "./src/console.rs" ) ) );
        assert!(  ret.iter().any( |x| x.path == PathBuf::from( "./src/lib.rs"     ) ) );
        assert!(  ret.iter().any( |x| x.path == PathBuf::from( "./src/matcher.rs" ) ) );
        assert!(  ret.iter().any( |x| x.path == PathBuf::from( "./src/util.rs"    ) ) );
        assert!( !ret.iter().any( |x| x.path == PathBuf::from( "./.git/config"    ) ) );
    }

    #[test]
    fn pipeline_finder_not_skip_vcs() {
        let mut finder = PipelineFinder::new();
        finder.skip_vcs = false;
        let ret = test( finder, "./".to_string() );

        assert!( ret.iter().any( |x| x.path == PathBuf::from( "./Cargo.toml"     ) ) );
        assert!( ret.iter().any( |x| x.path == PathBuf::from( "./src/ambr.rs"    ) ) );
        assert!( ret.iter().any( |x| x.path == PathBuf::from( "./src/ambs.rs"    ) ) );
        assert!( ret.iter().any( |x| x.path == PathBuf::from( "./src/console.rs" ) ) );
        assert!( ret.iter().any( |x| x.path == PathBuf::from( "./src/lib.rs"     ) ) );
        assert!( ret.iter().any( |x| x.path == PathBuf::from( "./src/matcher.rs" ) ) );
        assert!( ret.iter().any( |x| x.path == PathBuf::from( "./src/util.rs"    ) ) );
        assert!( ret.iter().any( |x| x.path == PathBuf::from( "./.git/config"    ) ) );
    }
}

