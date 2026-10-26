use std::collections::BTreeMap;
pub const END:u32=0x50000000;
fn aligned(size:u32)->Option<u32>{size.max(1).checked_add(15).map(|n|n&!15)}
pub fn allocate(cursor:&mut u32,free:&mut BTreeMap<u32,u32>,size:u32)->u32 {
 let Some(size)=aligned(size) else{return 0};
 if let Some((&p,&available))=free.iter().find(|(_,n)|**n>=size){free.remove(&p);if available>size{free.insert(p+size,available-size);}return p}
 let Some(end)=cursor.checked_add(size).filter(|n|*n<=END) else{return 0};let p=*cursor;*cursor=end;p
}
pub fn release(free:&mut BTreeMap<u32,u32>,mut p:u32,size:u32){
 let mut size=aligned(size).expect("invalid allocation size");
 if let Some((&left,&n))=free.range(..p).next_back(){assert!(left+n<=p,"overlapping heap release");if left+n==p{free.remove(&left);p=left;size+=n;}}
 if let Some((&right,&n))=free.range(p..).next(){assert!(p+size<=right,"overlapping heap release");if p+size==right{free.remove(&right);size+=n;}}
 assert!(free.insert(p,size).is_none());
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn reuse_and_coalesce(){let mut c=0x40000000;let mut f=BTreeMap::new();let a=allocate(&mut c,&mut f,17);let b=allocate(&mut c,&mut f,31);let d=allocate(&mut c,&mut f,1);release(&mut f,b,31);release(&mut f,a,17);release(&mut f,d,1);assert_eq!(f.get(&a),Some(&80));assert_eq!(allocate(&mut c,&mut f,64),a);assert_eq!(allocate(&mut c,&mut f,16),a+64);assert!(f.is_empty());assert_eq!(c,a+80);}
 #[test]fn exhaustion_preserves_state(){let mut c=END-16;let mut f=BTreeMap::new();assert_eq!(allocate(&mut c,&mut f,17),0);assert_eq!(c,END-16);assert_eq!(allocate(&mut c,&mut f,u32::MAX),0);assert_eq!(allocate(&mut c,&mut f,0),END-16);assert_eq!(c,END);assert_eq!(allocate(&mut c,&mut f,1),0);}
 #[test]fn repeated_temporary_buffers_do_not_grow_heap(){let mut c=0x40000000;let mut f=BTreeMap::new();for _ in 0..10000{let p=allocate(&mut c,&mut f,1536000);release(&mut f,p,1536000);}assert_eq!(c,0x40000000+1536000);}
}
